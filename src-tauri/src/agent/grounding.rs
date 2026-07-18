//! Two-layer system-instruction grounding, chained onto the base system prompt
//! before every LLM turn.
//!
//! - **Layer 1 — Global personalization** (`<config_dir>/config.json`): a JSON
//!   object with `instructions` (string or array of strings; `personalization`
//!   is accepted as an alias). Applies to *every* chat, like a Copilot-style
//!   personalization engine.
//! - **Layer 2 — Project workspace** (`<config_dir>/workspaces/<workspace_id>/
//!   finmodel.md`, falling back to `claude.md`): Markdown grounding, tools, and
//!   constraints unique to one project folder. Applied right *after* the global
//!   layer for chats in that workspace.
//!
//! The driver detects, reads, and chains these onto the base prompt so the LLM
//! always sees `base → global → project`.

use std::path::Path;

/// Read the global personalization block from `<config_dir>/config.json`.
///
/// Accepts `{ "instructions": "…" }` or `{ "personalization": "…" }`, where the
/// value is either a string or an array of strings (rendered as a bullet list).
/// Returns `None` when the file is absent, unparseable, or the block is empty.
pub fn read_global(config_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(config_dir.join("config.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let node = v.get("instructions").or_else(|| v.get("personalization"))?;
    let text = value_to_text(node);
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Read a project's grounding from `<config_dir>/projects/<project_id>/finmodel.md`
/// (falling back to `claude.md`). Returns `None` when the id is blank/unsafe or
/// the file is absent/empty. Layout matches the projects data model and stays
/// hand-editable.
pub fn read_project(config_dir: &Path, project_id: &str) -> Option<String> {
    let id = project_id.trim();
    if !is_valid_id(id) {
        return None;
    }
    let dir = config_dir.join("projects").join(id);
    let text = std::fs::read_to_string(dir.join("finmodel.md"))
        .or_else(|_| std::fs::read_to_string(dir.join("claude.md")))
        .ok()?;
    let trimmed = text.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Chain the base system prompt with the optional global + project layers, in
/// that precedence order. Present layers get a labelled section; absent layers
/// are skipped. Global precedes project so a project can refine — not silently
/// contradict — the user's global preferences.
pub fn chain(base: &str, global: Option<&str>, project: Option<&str>) -> String {
    let mut out = String::from(base);
    if let Some(g) = global.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n\n## Global personalization (applies to every chat)\n");
        out.push_str(g);
    }
    if let Some(p) = project.map(str::trim).filter(|s| !s.is_empty()) {
        out.push_str("\n\n## Project grounding (this project folder)\n");
        out.push_str(p);
    }
    out
}

/// Validate an id used as a path segment: non-empty and restricted to
/// `[A-Za-z0-9_-]`, so an IPC-supplied id can't traverse out of the config dir
/// (e.g. `..\..\evil`). UUIDs and slugs pass; path separators and dots are out.
pub fn is_valid_id(id: &str) -> bool {
    let t = id.trim();
    !t.is_empty()
        && t.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Path a project's grounding file lives at (for the setter command). Returns
/// `None` for a blank or unsafe project id.
pub fn project_file(config_dir: &Path, project_id: &str) -> Option<std::path::PathBuf> {
    if !is_valid_id(project_id) {
        return None;
    }
    Some(
        config_dir
            .join("projects")
            .join(project_id.trim())
            .join("finmodel.md"),
    )
}

fn value_to_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|i| i.as_str())
            .map(|s| format!("- {s}"))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("fm-grounding-{:x}", fastrand::u64(..)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn global_reads_string_instructions() {
        let d = tmp();
        std::fs::write(
            d.join("config.json"),
            r#"{ "instructions": "Always format tables in Markdown" }"#,
        )
        .unwrap();
        assert_eq!(
            read_global(&d).as_deref(),
            Some("Always format tables in Markdown")
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn global_reads_array_and_personalization_alias() {
        let d = tmp();
        std::fs::write(
            d.join("config.json"),
            r#"{ "personalization": ["Prefer USD", "Be concise"] }"#,
        )
        .unwrap();
        assert_eq!(
            read_global(&d).as_deref(),
            Some("- Prefer USD\n- Be concise")
        );
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn global_absent_or_empty_is_none() {
        let d = tmp();
        assert_eq!(read_global(&d), None); // no file
        std::fs::write(d.join("config.json"), r#"{ "instructions": "   " }"#).unwrap();
        assert_eq!(read_global(&d), None); // empty after trim
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn project_reads_finmodel_then_claude() {
        let d = tmp();
        let pdir = d.join("projects").join("p1");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("claude.md"), "benchmark against AMD").unwrap();
        // claude.md fallback when finmodel.md absent
        assert_eq!(
            read_project(&d, "p1").as_deref(),
            Some("benchmark against AMD")
        );
        // finmodel.md wins when present
        std::fs::write(pdir.join("finmodel.md"), "NVDA vs AMD/INTC").unwrap();
        assert_eq!(read_project(&d, "p1").as_deref(), Some("NVDA vs AMD/INTC"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn project_blank_id_or_missing_is_none() {
        let d = tmp();
        assert_eq!(read_project(&d, ""), None);
        assert_eq!(read_project(&d, "nope"), None);
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn project_rejects_path_traversal() {
        let d = tmp();
        assert!(!is_valid_id("../../etc"));
        assert!(!is_valid_id("..\\..\\evil"));
        assert!(!is_valid_id("a/b"));
        assert!(!is_valid_id("a b"));
        assert!(is_valid_id("ws-1_A9f"));
        assert_eq!(read_project(&d, "../../etc"), None);
        assert!(project_file(&d, "..\\evil").is_none());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn chain_orders_base_global_project() {
        let out = chain("BASE", Some("GLOBAL"), Some("PROJECT"));
        let gi = out.find("GLOBAL").unwrap();
        let pi = out.find("PROJECT").unwrap();
        let bi = out.find("BASE").unwrap();
        assert!(bi < gi && gi < pi, "order base<global<project: {out}");
        assert!(out.contains("Global personalization"));
        assert!(out.contains("Project grounding"));
    }

    #[test]
    fn chain_skips_absent_layers() {
        assert_eq!(chain("BASE", None, None), "BASE");
        let only_global = chain("BASE", Some("G"), None);
        assert!(only_global.contains("Global personalization"));
        assert!(!only_global.contains("Project workspace grounding"));
        // whitespace-only layers are treated as absent
        assert_eq!(chain("BASE", Some("  "), Some("\n")), "BASE");
    }
}
