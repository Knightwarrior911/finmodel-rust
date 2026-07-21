//! User-defined agents (the orchestrator's bench): named specialists the
//! analyst dispatches as true subagents via the `run_agent` tool — own
//! context, read-only tool belt, the user's chosen skills preloaded, and a
//! compact typed brief back to the parent. Independent agents dispatched in
//! one turn run in PARALLEL (the same proven child-loop machinery as
//! `delegate_analysis`).
//!
//! Stored exactly like skills — one Markdown file per agent under
//! `<config_dir>/agents/<name>.md` with frontmatter:
//!
//! ```md
//! ---
//! name: dd-reviewer
//! description: Red-teams deal documents for diligence gaps
//! skills: comparable-companies, precedent-transactions
//! ---
//! You are the diligence reviewer. For every claim ...
//! ```
//!
//! `skills:` is optional — listed skills are loaded from the user's skill
//! library and PRELOADED into the agent's system prompt at dispatch (no
//! tool round-trip needed); the agent also carries `use_skill` for the
//! rest of the library.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct AgentDef {
    pub name: String,
    pub description: String,
    /// Skill names preloaded into the agent's system prompt at dispatch.
    pub skills: Vec<String>,
    /// The agent's doctrine — its system-prompt body.
    pub body: String,
}

fn agents_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("agents")
}

/// Same naming contract as skills: filename-safe, human-typable.
fn is_valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty()
        && n.len() <= 64
        && n.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Parse an AGENT.md (frontmatter + body). None when malformed.
pub fn parse_agent(md: &str) -> Option<AgentDef> {
    let text = md.trim_start_matches('\u{feff}');
    let rest = text.strip_prefix("---")?;
    let mut fm = String::new();
    let mut body = String::new();
    let mut in_fm = true;
    for (i, line) in rest.lines().enumerate() {
        if in_fm && line.trim() == "---" {
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
        return None;
    }
    let mut name = None;
    let mut description = None;
    let mut skills: Vec<String> = Vec::new();
    for line in fm.lines() {
        let line = line.trim();
        if let Some(v) = line.strip_prefix("name:") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("description:") {
            description = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("skills:") {
            skills = v
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    let name = name.filter(|n| is_valid_name(n))?;
    let description = description.filter(|d| !d.is_empty())?;
    let body = body.trim().to_string();
    if body.is_empty() {
        return None;
    }
    Some(AgentDef {
        name,
        description,
        skills,
        body,
    })
}

/// Load one agent by name.
pub fn get_agent(config_dir: &Path, name: &str) -> Option<AgentDef> {
    if !is_valid_name(name) {
        return None;
    }
    let path = agents_dir(config_dir).join(format!("{}.md", name.trim()));
    std::fs::read_to_string(path).ok().and_then(|s| parse_agent(&s))
}

/// Raw AGENT.md text for the editor.
pub fn get_agent_md(config_dir: &Path, name: &str) -> Option<String> {
    if !is_valid_name(name) {
        return None;
    }
    std::fs::read_to_string(agents_dir(config_dir).join(format!("{}.md", name.trim()))).ok()
}

/// All agents, name-sorted.
pub fn list_agents(config_dir: &Path) -> Vec<AgentDef> {
    let Ok(entries) = std::fs::read_dir(agents_dir(config_dir)) else {
        return Vec::new();
    };
    let mut out: Vec<AgentDef> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map(|x| x == "md").unwrap_or(false))
        .filter_map(|e| std::fs::read_to_string(e.path()).ok())
        .filter_map(|s| parse_agent(&s))
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

/// Persist an agent file (full AGENT.md text; frontmatter name must match).
pub fn save_agent(config_dir: &Path, name: &str, content: &str) -> Result<PathBuf, String> {
    if !is_valid_name(name) {
        return Err("invalid agent name".into());
    }
    let parsed = parse_agent(content)
        .ok_or("content is not a valid AGENT.md (need name + description frontmatter and a body)")?;
    if parsed.name.trim() != name.trim() {
        return Err(format!(
            "frontmatter name `{}` must match file name `{}`",
            parsed.name, name
        ));
    }
    let dir = agents_dir(config_dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("{}.md", name.trim()));
    std::fs::write(&path, content).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Delete an agent by name. Missing file is not an error.
pub fn delete_agent(config_dir: &Path, name: &str) -> Result<(), String> {
    if !is_valid_name(name) {
        return Err("invalid agent name".into());
    }
    let path = agents_dir(config_dir).join(format!("{}.md", name.trim()));
    if path.exists() {
        std::fs::remove_file(&path).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// The system-prompt catalog block: how the orchestrator knows its bench.
pub fn catalog_block(agents: &[AgentDef]) -> Option<String> {
    if agents.is_empty() {
        return None;
    }
    let mut out = String::from(
        "## Your agents (dispatch with `run_agent`)\nUser-defined specialists that run as subagents in their own context and report back a brief. Dispatch INDEPENDENT agents in the SAME turn - they run in parallel.\n",
    );
    for a in agents {
        out.push_str(&format!("- **{}** — {}\n", a.name, a.description));
    }
    Some(out)
}

/// The dispatched agent's full system prompt: its doctrine, then each
/// preloaded skill verbatim, then the child ground rules (evidence
/// doctrine, brief format) — in that order so the agent's own voice leads.
pub fn agent_system_prompt(config_dir: &Path, def: &AgentDef, ground_rules: &str) -> String {
    let mut prompt = def.body.clone();
    for name in &def.skills {
        if let Some(skill) = crate::agent::skills::get_skill(config_dir, name) {
            prompt.push_str(&format!(
                "\n\n## Skill: {} ({})\n{}",
                skill.name, skill.description, skill.body
            ));
        } else {
            prompt.push_str(&format!(
                "\n\n(Note: the skill `{name}` listed for this agent no longer exists in the library.)"
            ));
        }
    }
    prompt.push_str("\n\n");
    prompt.push_str(ground_rules);
    prompt
}
/// The starter bench shipped with the app, seeded once into
/// `<config_dir>/agents/` at startup (see `run()` in lib.rs). Each file lives
/// in `src-tauri/agents/` and must parse + reference only real built-in
/// skills (both enforced by tests). Read-only research specialists.
const BUILTIN_AGENTS: &[(&str, &str)] = &[
    (
        "diligence-reviewer",
        include_str!("../../agents/diligence-reviewer.md"),
    ),
    (
        "comps-analyst",
        include_str!("../../agents/comps-analyst.md"),
    ),
    (
        "earnings-reviewer",
        include_str!("../../agents/earnings-reviewer.md"),
    ),
    (
        "credit-analyst",
        include_str!("../../agents/credit-analyst.md"),
    ),
    (
        "deal-screener",
        include_str!("../../agents/deal-screener.md"),
    ),
];

/// Marker that makes seeding one-shot per version: a user's later delete of a
/// built-in agent stays sticky across restarts.
const SEED_MARKER: &str = ".seeded_v1";

/// Seed the starter agents into `<config_dir>/agents/` exactly once. Existing
/// files are never overwritten (user edits win) and the marker keeps deletions
/// sticky. Best-effort: IO failures skip, never abort startup. Returns how
/// many files were written.
pub fn seed_builtin_agents(config_dir: &Path) -> usize {
    let dir = agents_dir(config_dir);
    if dir.join(SEED_MARKER).exists() {
        return 0;
    }
    if std::fs::create_dir_all(&dir).is_err() {
        return 0;
    }
    let mut written = 0;
    for (name, content) in BUILTIN_AGENTS {
        let path = dir.join(format!("{name}.md"));
        if !path.exists() && std::fs::write(&path, content).is_ok() {
            written += 1;
        }
    }
    let _ = std::fs::write(dir.join(SEED_MARKER), "v1\n");
    written
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("fm-agents-{:x}", fastrand::u64(..)));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const MD: &str = "---\nname: dd-reviewer\ndescription: Red-teams deal documents\nskills: alpha-skill, beta-skill\n---\nYou are the diligence reviewer. Challenge every claim.\n";

    #[test]
    fn parse_roundtrip_and_crud() {
        let dir = tmp();
        // Parse: name, description, skills list, body.
        let def = parse_agent(MD).unwrap();
        assert_eq!(def.name, "dd-reviewer");
        assert_eq!(def.skills, vec!["alpha-skill", "beta-skill"]);
        assert!(def.body.contains("Challenge every claim"));
        // Save + list + get + delete.
        save_agent(&dir, "dd-reviewer", MD).unwrap();
        assert_eq!(list_agents(&dir).len(), 1);
        assert_eq!(get_agent(&dir, "dd-reviewer").unwrap().description, "Red-teams deal documents");
        // Name mismatch and traversal names rejected.
        assert!(save_agent(&dir, "other-name", MD).is_err());
        assert!(save_agent(&dir, "../evil", MD).is_err());
        delete_agent(&dir, "dd-reviewer").unwrap();
        assert!(list_agents(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn malformed_agents_are_rejected_not_half_parsed() {
        assert!(parse_agent("no frontmatter at all").is_none());
        assert!(parse_agent("---\nname: x\n---\n").is_none(), "empty body");
        assert!(parse_agent("---\nname: bad name!\ndescription: d\n---\nbody").is_none());
        assert!(parse_agent("---\ndescription: d\n---\nbody").is_none(), "no name");
    }

    #[test]
    fn dispatch_prompt_preloads_skills_and_flags_missing_ones() {
        let dir = tmp();
        crate::agent::skills::save_skill(
            &dir,
            "alpha-skill",
            "---\nname: alpha-skill\ndescription: Alpha steps\n---\nStep 1: do alpha.\n",
        )
        .unwrap();
        let def = parse_agent(MD).unwrap();
        let p = agent_system_prompt(&dir, &def, "GROUND RULES HERE");
        // Agent's own doctrine leads.
        assert!(p.starts_with("You are the diligence reviewer"));
        // Listed + existing skill preloaded verbatim.
        assert!(p.contains("## Skill: alpha-skill"));
        assert!(p.contains("Step 1: do alpha."));
        // Listed but missing skill is flagged honestly, never silently dropped.
        assert!(p.contains("`beta-skill` listed for this agent no longer exists"));
        // Ground rules close the prompt.
        assert!(p.trim_end().ends_with("GROUND RULES HERE"));
        let _ = std::fs::remove_dir_all(&dir);
    }
    #[test]
    fn bundled_agents_parse_match_names_and_cite_real_skills() {
        let known: std::collections::HashSet<&str> =
            crate::agent::skills::builtin_skill_names().into_iter().collect();
        assert_eq!(BUILTIN_AGENTS.len(), 5, "the starter bench is five agents");
        for (name, content) in BUILTIN_AGENTS {
            let def = parse_agent(content)
                .unwrap_or_else(|| panic!("bundled agent `{name}` does not parse"));
            // Frontmatter name must match the file stem (seeding writes by stem).
            assert_eq!(&def.name, name, "agent `{name}` frontmatter name mismatch");
            assert!(!def.description.is_empty());
            assert!(!def.skills.is_empty(), "`{name}` should wire real skills");
            // Every referenced skill must exist — no typos, no silent
            // preload-degradation when a skill is renamed later.
            for s in &def.skills {
                assert!(
                    known.contains(s.as_str()),
                    "agent `{name}` references unknown skill `{s}`"
                );
            }
        }
    }

    #[test]
    fn seed_builtin_agents_is_one_shot_and_never_clobbers() {
        let dir = tmp();
        // A user's own comps-analyst exists BEFORE the first seed.
        let mine = "---\nname: comps-analyst\ndescription: my tweak\n---\nmy own doctrine\n";
        save_agent(&dir, "comps-analyst", mine).unwrap();
        // First run writes every OTHER agent, skips the pre-existing one.
        let n = seed_builtin_agents(&dir);
        assert_eq!(n, BUILTIN_AGENTS.len() - 1, "the user's file is not overwritten");
        assert_eq!(list_agents(&dir).len(), BUILTIN_AGENTS.len());
        assert_eq!(
            get_agent(&dir, "comps-analyst").unwrap().description,
            "my tweak",
            "the user's content survived the first seed"
        );
        // Second run: marker makes it a no-op, and a delete stays sticky.
        delete_agent(&dir, "credit-analyst").unwrap();
        let n2 = seed_builtin_agents(&dir);
        assert_eq!(n2, 0, "marker makes re-seeding a no-op");
        assert!(get_agent(&dir, "credit-analyst").is_none(), "deletion stays sticky");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
