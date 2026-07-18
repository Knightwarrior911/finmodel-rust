//! Decentralized SKILL.md system.
//!
//! A **skill** is a drop-in Markdown file at `<config_dir>/skills/<name>.md` with
//! YAML frontmatter and a Markdown body:
//!
//! ```text
//! ---
//! name: earnings-snapshot
//! description: When the user wants a company's latest annual earnings summary.
//! ---
//! 1. Call get_financials for the ticker (latest fiscal year).
//! 2. Report revenue, net income, and margin in a short table.
//! ```
//!
//! Frontmatter carries `name` + `description` (both required) and an optional
//! `parameters` (inline JSON Schema) so a skill can later be exposed as a typed
//! tool. The body is natural-language instructions the model follows.
//!
//! Discovery uses progressive disclosure: the catalog (name + description) is
//! injected into the system prompt so the model knows what exists; the full body
//! is small enough here that [`catalog_block`] includes it, capped, so no extra
//! round-trip is needed for a handful of user skills.

use std::path::{Path, PathBuf};

/// One parsed skill.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub description: String,
    /// Optional inline JSON Schema for a typed invocation (reserved).
    pub parameters: Option<String>,
    pub body: String,
}

/// Load a single skill by name from `<config_dir>/skills/<name>.md`.
pub fn get_skill(config_dir: &Path, name: &str) -> Option<Skill> {
    if !is_valid_name(name) {
        return None;
    }
    let path = skills_dir(config_dir).join(format!("{}.md", name.trim()));
    let text = std::fs::read_to_string(path).ok()?;
    parse_skill(&text)
}

/// Validate a skill name used as a filename: non-empty, `[A-Za-z0-9_-]`, so an
/// IPC-supplied name can't traverse the filesystem.
pub fn is_valid_name(name: &str) -> bool {
    let t = name.trim();
    !t.is_empty()
        && t.len() <= 64
        && t.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn skills_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("skills")
}

/// Parse a SKILL.md document. Returns `None` if the frontmatter is missing
/// `name`/`description` or the fences are malformed.
pub fn parse_skill(md: &str) -> Option<Skill> {
    let text = md.trim_start_matches('\u{feff}');
    let rest = text.strip_prefix("---")?;
    // Frontmatter is everything up to the next line that is exactly `---`.
    let mut fm = String::new();
    let mut body = String::new();
    let mut in_fm = true;
    for (i, line) in rest.lines().enumerate() {
        if in_fm && (line.trim() == "---") {
            in_fm = false;
            continue;
        }
        if in_fm {
            if i > 0 || !line.is_empty() {
                fm.push_str(line);
                fm.push('\n');
            }
        } else {
            body.push_str(line);
            body.push('\n');
        }
    }
    if in_fm {
        return None; // never closed the frontmatter
    }
    let mut name = None;
    let mut description = None;
    let mut parameters = None;
    for line in fm.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(unquote(v));
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(unquote(v));
        } else if let Some(v) = line.strip_prefix("parameters:") {
            let v = v.trim();
            if !v.is_empty() {
                parameters = Some(v.to_string());
            }
        }
    }
    let name = name.filter(|s| !s.is_empty())?;
    let description = description.filter(|s| !s.is_empty())?;
    Some(Skill {
        name,
        description,
        parameters,
        body: body.trim().to_string(),
    })
}

fn unquote(s: &str) -> String {
    let t = s.trim();
    let t = t.strip_prefix('"').unwrap_or(t);
    let t = t.strip_suffix('"').unwrap_or(t);
    let t = t.strip_prefix('\'').unwrap_or(t);
    let t = t.strip_suffix('\'').unwrap_or(t);
    t.trim().to_string()
}

/// List all skills in `<config_dir>/skills`, sorted by name. Invalid files are
/// skipped, not fatal.
pub fn list_skills(config_dir: &Path) -> Vec<Skill> {
    let dir = skills_dir(config_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<Skill> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| parse_skill(&s))
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Persist a skill file. `content` is the full SKILL.md text; it must parse and
/// its frontmatter `name` must match `name` (the filename stem). Returns the
/// path written.
pub fn save_skill(config_dir: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
    if !is_valid_name(name) {
        return Err("invalid skill name".into());
    }
    let parsed = parse_skill(content).ok_or("content is not a valid SKILL.md (need name + description frontmatter)")?;
    if parsed.name.trim() != name.trim() {
        return Err(format!(
            "frontmatter name `{}` must match file name `{}`",
            parsed.name, name
        ));
    }
    let dir = skills_dir(config_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.md", name.trim()));
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Delete a skill file by name. Missing file is not an error.
pub fn delete_skill(config_dir: &Path, name: &str) -> Result<(), String> {
    if !is_valid_name(name) {
        return Err("invalid skill name".into());
    }
    let path = skills_dir(config_dir).join(format!("{}.md", name.trim()));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Render the skills catalog for system-prompt injection, capped at
/// [`CATALOG_BUDGET`]. Returns `None` when there are no skills.
pub fn catalog_block(skills: &[Skill]) -> Option<String> {
    if skills.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## Skills (reusable playbooks)\nWhen the user's request matches one, call the `use_skill` tool with its `name` to load the full steps, then follow them.\n",
    );
    for s in skills {
        out.push_str(&format!("- **{}** — {}\n", s.name, s.description));
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("fm-skills-{:x}", fastrand::u64(..)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const SAMPLE: &str = "---\nname: earnings-snapshot\ndescription: Latest annual earnings summary.\n---\n1. Call get_financials.\n2. Report revenue and net income.";

    #[test]
    fn parses_frontmatter_and_body() {
        let s = parse_skill(SAMPLE).unwrap();
        assert_eq!(s.name, "earnings-snapshot");
        assert_eq!(s.description, "Latest annual earnings summary.");
        assert!(s.body.starts_with("1. Call get_financials"));
        assert!(s.parameters.is_none());
    }

    #[test]
    fn parses_quoted_and_parameters() {
        let md = "---\nname: \"quote-check\"\ndescription: 'Check a live quote.'\nparameters: {\"type\":\"object\",\"properties\":{\"ticker\":{\"type\":\"string\"}}}\n---\nBody here.";
        let s = parse_skill(md).unwrap();
        assert_eq!(s.name, "quote-check");
        assert_eq!(s.description, "Check a live quote.");
        assert_eq!(
            s.parameters.as_deref(),
            Some("{\"type\":\"object\",\"properties\":{\"ticker\":{\"type\":\"string\"}}}")
        );
    }

    #[test]
    fn rejects_missing_fields_or_fences() {
        assert!(parse_skill("no frontmatter here").is_none());
        assert!(parse_skill("---\nname: x\n---\nbody").is_none()); // no description
        assert!(parse_skill("---\ndescription: y\nbody").is_none()); // unclosed + no name
    }

    #[test]
    fn save_list_delete_roundtrip() {
        let d = tmp();
        save_skill(&d, "earnings-snapshot", SAMPLE).unwrap();
        let skills = list_skills(&d);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "earnings-snapshot");
        delete_skill(&d, "earnings-snapshot").unwrap();
        assert_eq!(list_skills(&d).len(), 0);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn save_rejects_name_mismatch_and_traversal() {
        let d = tmp();
        assert!(save_skill(&d, "a", SAMPLE).is_err()); // frontmatter name != "a"
        assert!(save_skill(&d, "../evil", SAMPLE).is_err()); // traversal
        assert!(!is_valid_name("../evil"));
        assert!(!is_valid_name("a b"));
        assert!(is_valid_name("earnings-snapshot"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn catalog_block_lists_names_and_descriptions() {
        assert!(catalog_block(&[]).is_none());
        let s = parse_skill(SAMPLE).unwrap();
        let block = catalog_block(std::slice::from_ref(&s)).unwrap();
        assert!(block.contains("earnings-snapshot"));
        assert!(block.contains("Latest annual earnings summary."));
        assert!(block.contains("use_skill"));
        // Bodies are NOT injected (progressive disclosure via use_skill).
        assert!(!block.contains("Report revenue and net income"));
    }
}
