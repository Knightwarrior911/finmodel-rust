//! LLM calling infrastructure for filing extraction.
//!
//! Ported from `src/extractor.py` — `_llm_complete()` and `_llm_complete_via_cli()`.
//! Supports:
//! - Claude Code CLI (`claude -p`) — fallback when no API key is set
//! - Provider API via HTTP (DeepSeek, Anthropic) when env keys are set
//!
//! # Important
//! On Windows, `claude` is a `.CMD` file and must be invoked via `cmd /c`.
//! The system prompt is written to a temp file to avoid shell quoting issues,
//! and user text is piped via stdin.

use std::io::Write;
use std::process::{Command, Stdio};

/// Errors from LLM operations.
#[derive(Debug, thiserror::Error)]
pub enum LlmError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("Claude CLI exited with code {rc}: {stderr}")]
    CliError { rc: i32, stderr: String },
    #[error("UTF-8 decode error: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),
}

/// Call an LLM with the given system and user prompts.
///
/// Provider selection (matches Python extractor.py):
///   1. `DEEPSEEK_API_KEY` set → DeepSeek (openai-compatible)
///   2. `ANTHROPIC_API_KEY` set → Anthropic SDK
///   3. Neither → Claude Code CLI (`claude -p`)
///
/// Model override: `FINMODEL_LLM_MODEL` env var.
pub fn llm_complete(system_text: &str, user_text: &str, max_tokens: u32) -> Result<String, LlmError> {
    let deepseek_key = std::env::var("DEEPSEEK_API_KEY").unwrap_or_default();
    let anthropic_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();

    if !deepseek_key.trim().is_empty() {
        return llm_complete_deepseek(system_text, user_text, max_tokens);
    }
    if !anthropic_key.trim().is_empty() {
        return llm_complete_anthropic(system_text, user_text, max_tokens);
    }
    llm_complete_via_cli(system_text, user_text)
}

/// Build the CLI args shared across both platforms.
fn claude_args(sys_path: &str, model: &str) -> Vec<String> {
    vec![
        "--model".into(),
        model.into(),
        "--system-prompt-file".into(),
        sys_path.into(),
        "--output-format".into(),
        "text".into(),
        "-p".into(),
        "Process the piped input per the system instructions and return only the requested JSON.".into(),
    ]
}

/// Spawn a subprocess, pipe user_text to its stdin, and collect output.
fn spawn_and_pipe(
    mut child: std::process::Child,
    user_text: &str,
) -> Result<(i32, String, String), LlmError> {
    // Write user text to stdin
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(user_text.as_bytes());
        // Drop stdin handle so the child can finish reading
        drop(stdin);
    }

    let output = child.wait_with_output()?;
    let out_raw = String::from_utf8_lossy(&output.stdout).to_string();
    let err_raw = String::from_utf8_lossy(&output.stderr).to_string();
    let rc = output.status.code().unwrap_or(-1);
    Ok((rc, out_raw, err_raw))
}

/// Write system prompt to a temp file and return its path.
fn write_sys_prompt(system_text: &str) -> Result<std::path::PathBuf, LlmError> {
    let tmp_dir = std::env::temp_dir();
    let sys_path = tmp_dir.join(format!("fm_extract_sys_{}.txt", std::process::id()));
    let mut f = std::fs::File::create(&sys_path)?;
    f.write_all(system_text.as_bytes())?;
    Ok(sys_path)
}

/// Call Claude via the Claude Code CLI.
///
/// Ported from `_llm_complete_via_cli()` in `src/extractor.py`.
fn llm_complete_via_cli(system_text: &str, user_text: &str) -> Result<String, LlmError> {
    let sys_path = write_sys_prompt(system_text)?;
    let model = std::env::var("FINMODEL_LLM_MODEL").unwrap_or_else(|_| "opus".to_string());

    let result = if cfg!(target_os = "windows") {
        let args = claude_args(&sys_path.to_string_lossy(), &model);
        let child = Command::new("cmd")
            .arg("/c")
            .arg("claude")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        spawn_and_pipe(child, user_text)?
    } else {
        let args = claude_args(&sys_path.to_string_lossy(), &model);
        let child = Command::new("claude")
            .args(&args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        spawn_and_pipe(child, user_text)?
    };

    // Always clean up temp file
    let _ = std::fs::remove_file(&sys_path);

    let (rc, out_raw, err_raw) = result;

    if rc != 0 {
        let err_trimmed: String = err_raw.chars().take(400).collect();
        return Err(LlmError::CliError { rc, stderr: err_trimmed });
    }

    let out = out_raw.trim().to_string();
    // Strip markdown code fences if present (matches Python behavior)
    Ok(strip_code_fences(&out))
}

/// Remove markdown code fences from LLM output.
/// Matches `_llm_complete_via_cli` lines 121-127 in Python.
fn strip_code_fences(s: &str) -> String {
    let s = s.trim();
    if s.starts_with("```") {
        let lines: Vec<&str> = s.lines().collect();
        if lines.len() <= 1 {
            return String::new();
        }
        let mut inner: Vec<&str> = lines[1..].to_vec();
        if let Some(last) = inner.last() {
            if last.trim() == "```" {
                inner.pop();
            }
        }
        inner.join("\n").trim().to_string()
    } else {
        s.to_string()
    }
}

/// Call DeepSeek API (openai-compatible).
fn llm_complete_deepseek(_system_text: &str, _user_text: &str, _max_tokens: u32) -> Result<String, LlmError> {
    Err(LlmError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "DeepSeek API caller not yet implemented; use Claude CLI or set ANTHROPIC_API_KEY",
    )))
}

/// Call Anthropic API directly.
fn llm_complete_anthropic(_system_text: &str, _user_text: &str, _max_tokens: u32) -> Result<String, LlmError> {
    Err(LlmError::Io(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Anthropic API caller not yet implemented; use Claude CLI",
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_code_fences_plain_text() {
        let input = "{\"key\": \"value\"}";
        assert_eq!(strip_code_fences(input), input);
    }

    #[test]
    fn test_strip_code_fences_with_fences() {
        let input = "```json\n{\"key\": \"value\"}\n```";
        let expected = "{\"key\": \"value\"}";
        assert_eq!(strip_code_fences(input), expected);
    }

    #[test]
    fn test_strip_code_fences_no_lang() {
        let input = "```\nplain text\n```";
        assert_eq!(strip_code_fences(input), "plain text");
    }

    #[test]
    fn test_strip_code_fences_trailing_newline() {
        let input = "```\nhello\n```\n";
        assert_eq!(strip_code_fences(input), "hello");
    }

    #[test]
    fn test_strip_code_fences_empty_fence() {
        let input = "```";
        assert_eq!(strip_code_fences(input), "");
    }
}
