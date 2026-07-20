//! Data room review (`analyze_data_room`): point the analyst at a deal
//! folder — subfolders, dozens of files — ask questions, and get answers
//! where every finding traces to an exact file, page, and verbatim quote.
//!
//! Traceability is enforced, not promised:
//! - the model answers from NUMBERED excerpts and cites excerpt numbers,
//!   never file names — we resolve the number back to (file, page), so a
//!   finding can never point at the wrong document;
//! - every quote is checked character-for-character (whitespace-normalized)
//!   against the excerpt it cites; failures are marked unverified in the
//!   card instead of silently trusted.
//!
//! Security shape: the tool runs under `Risk::LocalRead` — it PAUSES for
//! the user's go-ahead before reading a user-named path (the artifact
//! registry stays the only auto-run door to local files). Extraction is
//! panic-safe per file; one corrupt PDF skips, never crashes the room.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Caps: a data room review is a bounded pass, not an unbounded crawl.
const MAX_FILES: usize = 60;
const MAX_QUESTIONS: usize = 6;
const MAX_FILE_BYTES: u64 = 40_000_000; // skip anything over 40 MB
const CHUNK_TARGET: usize = 1600; // chars per excerpt handed to the model
const TOP_K: usize = 8; // excerpts per question
const MAX_FINDINGS: usize = 4;

/// One readable file in the room, split into pages (non-PDF = one page).
pub(crate) struct RoomFile {
    pub abs: PathBuf,
    pub rel: String,
    pub pages: Vec<String>,
}

/// One retrievable excerpt with exact provenance.
#[derive(Clone, Debug)]
pub(crate) struct Chunk {
    pub file_idx: usize,
    /// 1-based page number within the file (1 for single-page formats).
    pub page: usize,
    pub text: String,
}

/// Walk the room: readable files in, everything else recorded as skipped
/// (the card says what was NOT read — silent gaps are how audits fail).
pub(crate) fn walk_room(root: &Path) -> (Vec<PathBuf>, Vec<String>) {
    // Junctions/symlinks are NEVER followed: a link inside the approved
    // room pointing at C:\Users would silently widen the user's consent.
    // Depth-capped for the same reason (rooms are shallow; loops are not).
    const MAX_DEPTH: usize = 8;
    let mut files = Vec::new();
    let mut skipped = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            skipped.push(format!("{} (unreadable folder)", dir.display()));
            continue;
        };
        let mut names: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
        names.sort(); // deterministic room order
        for p in names {
            let is_link = std::fs::symlink_metadata(&p)
                .map(|m| m.file_type().is_symlink())
                .unwrap_or(true);
            if is_link {
                skipped.push(format!("{} (link — not followed)", rel_of(root, &p)));
                continue;
            }
            if p.is_dir() {
                if depth + 1 > MAX_DEPTH {
                    skipped.push(format!("{} (deeper than {MAX_DEPTH} levels)", rel_of(root, &p)));
                } else {
                    stack.push((p, depth + 1));
                }
                continue;
            }
            let ext = p
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_ascii_lowercase())
                .unwrap_or_default();
            let readable = matches!(ext.as_str(), "pdf" | "txt" | "md" | "htm" | "html" | "csv");
            if !readable {
                if !ext.is_empty() {
                    skipped.push(format!("{} (.{ext} not supported yet)", rel_of(root, &p)));
                }
                continue;
            }
            let too_big = std::fs::metadata(&p).map(|m| m.len() > MAX_FILE_BYTES).unwrap_or(true);
            if too_big {
                skipped.push(format!("{} (over the size cap)", rel_of(root, &p)));
                continue;
            }
            files.push(p);
        }
    }
    if files.len() > MAX_FILES {
        skipped.push(format!(
            "{} more files beyond the {MAX_FILES}-file cap (narrow the folder or ask again on a subfolder)",
            files.len() - MAX_FILES
        ));
        files.truncate(MAX_FILES);
    }
    (files, skipped)
}

fn rel_of(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Extract one file into pages. HTML gets a crude tag strip (good enough
/// for retrieval + quoting; the click-through opens the real file).
pub(crate) fn extract_file(root: &Path, p: &Path) -> Result<RoomFile, String> {
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    let pages: Vec<String> = if ext == "pdf" {
        fm_extract::pdf_pages(&p.to_string_lossy()).map_err(|e| e.to_string())?
    } else {
        let raw = std::fs::read_to_string(p).map_err(|e| e.to_string())?;
        let text = if ext == "htm" || ext == "html" { strip_tags(&raw) } else { raw };
        vec![text]
    };
    Ok(RoomFile {
        abs: p.to_path_buf(),
        rel: rel_of(root, p),
        pages,
    })
}

/// Crude tag strip for HTML corpus files (retrieval-grade, not a renderer).
pub(crate) fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut in_script = false;
    let lower = html.to_ascii_lowercase();
    let bytes = html.char_indices().collect::<Vec<_>>();
    let mut i = 0;
    while i < bytes.len() {
        let (pos, c) = bytes[i];
        if !in_tag && c == '<' {
            in_tag = true;
            if lower[pos..].starts_with("<script") || lower[pos..].starts_with("<style") {
                in_script = true;
            }
            if in_script && (lower[pos..].starts_with("</script") || lower[pos..].starts_with("</style")) {
                in_script = false;
            }
            i += 1;
            continue;
        }
        if in_tag {
            if c == '>' {
                in_tag = false;
                out.push(' ');
            }
            i += 1;
            continue;
        }
        if !in_script {
            out.push(c);
        }
        i += 1;
    }
    out
}

/// Split pages into retrieval chunks around paragraph boundaries.
pub(crate) fn chunk_room(files: &[RoomFile]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    for (fi, f) in files.iter().enumerate() {
        for (pi, page) in f.pages.iter().enumerate() {
            let text = page.trim();
            if text.is_empty() {
                continue;
            }
            if text.len() <= CHUNK_TARGET {
                chunks.push(Chunk { file_idx: fi, page: pi + 1, text: text.to_string() });
                continue;
            }
            let mut cur = String::new();
            for para in text.split("\n\n") {
                if !cur.is_empty() && cur.len() + para.len() + 2 > CHUNK_TARGET {
                    chunks.push(Chunk { file_idx: fi, page: pi + 1, text: cur.clone() });
                    cur.clear();
                }
                if para.len() > CHUNK_TARGET {
                    // Pathological paragraph: hard-split on char boundaries.
                    let mut rest = para;
                    while rest.len() > CHUNK_TARGET {
                        let mut cut = CHUNK_TARGET;
                        while cut > 0 && !rest.is_char_boundary(cut) {
                            cut -= 1;
                        }
                        chunks.push(Chunk { file_idx: fi, page: pi + 1, text: rest[..cut].to_string() });
                        rest = &rest[cut..];
                    }
                    cur.push_str(rest);
                } else {
                    if !cur.is_empty() {
                        cur.push_str("\n\n");
                    }
                    cur.push_str(para);
                }
            }
            if !cur.trim().is_empty() {
                chunks.push(Chunk { file_idx: fi, page: pi + 1, text: cur });
            }
        }
    }
    chunks
}

/// Lowercase alphanumeric tokens (numbers keep separators stripped so
/// "96,307" matches "96307" queries and vice versa).
pub(crate) fn tokenize(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in s.chars() {
        if c.is_alphanumeric() {
            cur.extend(c.to_lowercase());
        } else if c == ',' && !cur.is_empty() && cur.chars().all(|c| c.is_ascii_digit()) {
            // digit group separator: keep the number together
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// BM25 ranking of chunks for one query. Small, deterministic, and plenty
/// for a bounded room (ties broken by chunk order = room order).
pub(crate) fn bm25_top_k(chunks: &[Chunk], query: &str, k: usize) -> Vec<usize> {
    const K1: f64 = 1.5;
    const B: f64 = 0.75;
    let docs: Vec<Vec<String>> = chunks.iter().map(|c| tokenize(&c.text)).collect();
    let n = docs.len();
    if n == 0 {
        return Vec::new();
    }
    let avg_len = docs.iter().map(|d| d.len()).sum::<usize>() as f64 / n as f64;
    let q_terms: std::collections::BTreeSet<String> = tokenize(query).into_iter().collect();
    // Document frequencies.
    let mut df: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for d in &docs {
        let uniq: std::collections::HashSet<&str> = d.iter().map(|s| s.as_str()).collect();
        for t in uniq {
            if q_terms.contains(t) {
                *df.entry(t).or_default() += 1;
            }
        }
    }
    let mut scored: Vec<(f64, usize)> = docs
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let len = d.len() as f64;
            let mut tf: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
            for t in d {
                if q_terms.contains(t.as_str()) {
                    *tf.entry(t.as_str()).or_default() += 1;
                }
            }
            let mut score = 0.0;
            for (t, &f) in &tf {
                let dfi = *df.get(*t).unwrap_or(&0) as f64;
                let idf = (((n as f64 - dfi + 0.5) / (dfi + 0.5)) + 1.0).ln();
                let f = f as f64;
                score += idf * (f * (K1 + 1.0)) / (f + K1 * (1.0 - B + B * len / avg_len.max(1.0)));
            }
            (score, i)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .filter(|(s, _)| *s > 0.0)
        .take(k)
        .map(|(_, i)| i)
        .collect()
}

/// Whitespace-normalized verbatim check: does `quote` appear in `text`?
pub(crate) fn quote_verified(text: &str, quote: &str) -> bool {
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
    let q = norm(quote);
    if q.len() < 4 {
        return false;
    }
    norm(text).contains(&q)
}

const ROOM_PROMPT: &str = "You are reviewing a private deal data room. Answer the question using ONLY the numbered excerpts provided. Reply with STRICT JSON: {\"answer\": \"...\", \"findings\": [{\"excerpt\": n, \"quote\": \"...\"}]} where each finding cites the excerpt NUMBER it came from and quotes the supporting text COPIED CHARACTER-FOR-CHARACTER from that excerpt (max 4 findings, shortest quote that proves the answer). Numbers, dates, and names must come from the excerpts - never from memory. If the excerpts do not answer the question, say exactly that in \"answer\" and return an empty findings array.";

/// Answer one question from ranked excerpts. Returns the per-question card
/// fragment; failures degrade to an honest \"couldn't check\" entry.
async fn answer_question(
    app: &tauri::AppHandle,
    cfg: &fm_extract::LlmConfig,
    conversation_id: &str,
    run: &str,
    cancel: &tokio_util::sync::CancellationToken,
    files: &[RoomFile],
    chunks: &[Chunk],
    question: &str,
) -> Value {
    let top = bm25_top_k(chunks, question, TOP_K);
    if top.is_empty() {
        return json!({
            "question": question,
            "answer": "Nothing in the readable files matches this question.",
            "findings": [],
        });
    }
    let excerpts = build_excerpts(&top, chunks, files);
    let msgs = vec![
        json!({ "role": "system", "content": ROOM_PROMPT }),
        json!({ "role": "user", "content": format!("Question: {question}\n\nExcerpts:\n{excerpts}") }),
    ];
    let req = crate::commands::chat::build_chat_request(&cfg.model, &msgs, &[], true, false);
    let acc = match crate::commands::chat::stream_completion_for_agent(
        app,
        conversation_id,
        run,
        cfg,
        &req,
        cancel,
        std::time::Duration::from_secs(60),
    )
    .await
    {
        Ok(acc) => acc,
        Err(e) => {
            return json!({
                "question": question,
                "answer": format!("Couldn't check this one ({e})."),
                "findings": [],
            });
        }
    };
    let (answer, findings) = resolve_findings(&acc.content, &top, chunks, files);
    json!({
        "question": question,
        "answer": answer,
        "findings": findings,
    })
}

/// Numbered excerpt block handed to the model — the numbers are the ONLY
/// citation vocabulary it gets.
pub(crate) fn build_excerpts(top: &[usize], chunks: &[Chunk], files: &[RoomFile]) -> String {
    top.iter()
        .enumerate()
        .map(|(n, &ci)| {
            let c = &chunks[ci];
            format!(
                "[{}] {} - page {}\n{}\n",
                n + 1,
                files[c.file_idx].rel,
                c.page,
                c.text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// The brittle leg, kept pure and tested: model reply -> (answer, findings).
/// Excerpt numbers resolve back to (file, page) HERE — the model never names
/// files, so it can never misattribute; every quote is verified verbatim.
/// A reply that isn't the contract JSON degrades to prose with no findings.
pub(crate) fn resolve_findings(
    content: &str,
    top: &[usize],
    chunks: &[Chunk],
    files: &[RoomFile],
) -> (String, Vec<Value>) {
    let parsed: Option<Value> = content
        .find('{')
        .and_then(|s| content.rfind('}').map(|e| (s, e)))
        .and_then(|(s, e)| serde_json::from_str(&content[s..=e]).ok());
    let Some(v) = parsed else {
        return (content.trim().to_string(), Vec::new());
    };
    let findings: Vec<Value> = v["findings"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .take(MAX_FINDINGS)
                .filter_map(|f| {
                    let quote = f["quote"].as_str().unwrap_or("").trim();
                    let n = f["excerpt"].as_u64().unwrap_or(0) as usize;
                    if quote.is_empty() || n == 0 || n > top.len() {
                        return None;
                    }
                    let c = &chunks[top[n - 1]];
                    let verified = quote_verified(&c.text, quote);
                    Some(json!({
                        "file": files[c.file_idx].rel,
                        "path": files[c.file_idx].abs.to_string_lossy(),
                        "page": c.page,
                        "quote": quote,
                        "verified": verified,
                    }))
                })
                .collect()
        })
        .unwrap_or_default();
    (
        v["answer"].as_str().unwrap_or("").trim().to_string(),
        findings,
    )
}

/// Run the full data room review. Returns `(model_summary, card)`.
pub(crate) async fn run_data_room(
    app: &tauri::AppHandle,
    cfg: &fm_extract::LlmConfig,
    root_str: &str,
    questions: &[String],
    conversation_id: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(String, Value), String> {
    if cfg.api_key.trim().is_empty() {
        return Err("a data room review needs the OpenRouter key configured in Settings".into());
    }
    let root = PathBuf::from(root_str.trim());
    if !root.is_dir() {
        return Err(format!("`{root_str}` is not a folder I can open"));
    }
    let questions: Vec<&String> = questions.iter().take(MAX_QUESTIONS).collect();
    if questions.is_empty() {
        return Err("give me at least one question to take into the room".into());
    }
    let (paths, mut skipped) = walk_room(&root);
    if paths.is_empty() {
        return Err("that folder has no readable documents (PDF, txt, md, html, csv)".into());
    }
    let mut files: Vec<RoomFile> = Vec::new();
    for p in &paths {
        if cancel.is_cancelled() {
            return Err("data room review cancelled".into());
        }
        match extract_file(&root, p) {
            Ok(f) if f.pages.iter().any(|pg| !pg.trim().is_empty()) => files.push(f),
            Ok(f) => skipped.push(format!("{} (no extractable text)", f.rel)),
            Err(e) => skipped.push(format!("{} ({e})", rel_of(&root, p))),
        }
    }
    if files.is_empty() {
        return Err("none of the files yielded readable text".into());
    }
    let chunks = chunk_room(&files);
    let run = format!("dataroom:{}", uuid_ish());
    let mut q_cards: Vec<Value> = Vec::new();
    for q in &questions {
        if cancel.is_cancelled() {
            return Err("data room review cancelled".into());
        }
        q_cards.push(
            answer_question(app, cfg, conversation_id, &run, cancel, &files, &chunks, q).await,
        );
    }
    let verified: usize = q_cards
        .iter()
        .flat_map(|q| q["findings"].as_array().cloned().unwrap_or_default())
        .filter(|f| f["verified"] == true)
        .count();
    let total_findings: usize = q_cards
        .iter()
        .map(|q| q["findings"].as_array().map(|a| a.len()).unwrap_or(0))
        .sum();
    let card = json!({
        "type": "data_room",
        "root": root.to_string_lossy(),
        "file_count": files.len(),
        "skipped": skipped,
        "questions": q_cards,
        "verified": verified,
        "findings": total_findings,
    });
    // The model's view: answers + provenance lines, compact.
    let mut text = format!(
        "Data room review of {} ({} readable files, {} findings, {} verified verbatim):\n",
        root.display(),
        files.len(),
        total_findings,
        verified
    );
    for q in &q_cards {
        text.push_str(&format!(
            "\nQ: {}\nA: {}\n",
            q["question"].as_str().unwrap_or(""),
            q["answer"].as_str().unwrap_or("")
        ));
        for f in q["findings"].as_array().cloned().unwrap_or_default() {
            text.push_str(&format!(
                "   [{} p.{}{}] \"{}\"\n",
                f["file"].as_str().unwrap_or(""),
                f["page"],
                if f["verified"] == true { "" } else { " - UNVERIFIED" },
                f["quote"].as_str().unwrap_or("")
            ));
        }
    }
    if !skipped.is_empty() {
        text.push_str(&format!("\nNot read: {}\n", skipped.join("; ")));
    }
    Ok((text, card))
}

fn uuid_ish() -> String {
    let mut b = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut b);
    fm_agent::ids::format_uuid_v4(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn room(files: &[(&str, &str)]) -> (Vec<RoomFile>, Vec<Chunk>) {
        let fs: Vec<RoomFile> = files
            .iter()
            .map(|(rel, text)| RoomFile {
                abs: PathBuf::from(format!("C:/room/{rel}")),
                rel: rel.to_string(),
                pages: vec![text.to_string()],
            })
            .collect();
        let chunks = chunk_room(&fs);
        (fs, chunks)
    }

    #[test]
    fn bm25_ranks_the_relevant_file_first() {
        let (_, chunks) = room(&[
            ("legal/nda.txt", "Mutual non-disclosure agreement between the parties."),
            ("fin/model.txt", "FY2025 revenue was 96,307 thousand USD with EBITDA margin of 31%."),
            ("hr/handbook.txt", "Vacation policy and onboarding checklist."),
        ]);
        let top = bm25_top_k(&chunks, "what was FY2025 revenue?", 3);
        assert_eq!(top[0], 1, "the financials chunk must rank first");
        // Irrelevant chunks with zero query-term overlap are dropped entirely.
        assert!(top.len() < 3);
    }

    #[test]
    fn quotes_verify_whitespace_normalized_and_reject_paraphrase() {
        let text = "Revenue for the year\n  was  96,307 thousand USD.";
        assert!(quote_verified(text, "revenue for the year was 96,307"));
        assert!(!quote_verified(text, "revenue was approximately 96 million"));
        assert!(!quote_verified(text, "was")); // too short to prove anything
    }

    #[test]
    fn chunking_keeps_page_provenance_and_splits_long_pages() {
        let long = "para one. ".repeat(50) + "\n\n" + &"para two. ".repeat(250);
        let f = RoomFile {
            abs: PathBuf::from("C:/room/a.pdf"),
            rel: "a.pdf".into(),
            pages: vec!["short page".into(), long],
        };
        let chunks = chunk_room(&[f]);
        assert!(chunks.len() >= 3, "long page split: {}", chunks.len());
        assert_eq!(chunks[0].page, 1);
        assert!(chunks[1..].iter().all(|c| c.page == 2), "page provenance survives splits");
        assert!(chunks.iter().all(|c| c.text.len() <= CHUNK_TARGET + 2));
    }

    #[test]
    fn walk_reports_unsupported_and_caps_honestly() {
        let dir = std::env::temp_dir().join(format!("fm_room_{}", std::process::id()));
        let sub = dir.join("legal");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(dir.join("a.txt"), "alpha").unwrap();
        std::fs::write(sub.join("b.md"), "beta").unwrap();
        std::fs::write(sub.join("c.docx"), "not yet").unwrap();
        let (files, skipped) = walk_room(&dir);
        assert_eq!(files.len(), 2);
        assert!(skipped.iter().any(|s| s.contains(".docx not supported")), "{skipped:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_findings_enforces_the_citation_contract() {
        let (files, chunks) = room(&[
            ("fin/accounts.txt", "Revenue for fiscal year 2025 was 96,307 thousand USD."),
            ("legal/nda.txt", "Mutual non-disclosure agreement."),
        ]);
        let top = vec![0usize, 1];
        // Good citation: excerpt 1 resolves to the FILE WE chose; verbatim
        // quote verifies. Out-of-range excerpt 9 is dropped, paraphrase is
        // kept but marked unverified.
        let reply = r#"noise {"answer": "FY2025 revenue was 96,307 thousand USD.", "findings": [
            {"excerpt": 1, "quote": "Revenue for fiscal year 2025 was 96,307 thousand USD."},
            {"excerpt": 1, "quote": "revenue was roughly 96 million"},
            {"excerpt": 9, "quote": "ghost"},
            {"excerpt": 0, "quote": "ghost"}
        ]} trailing"#;
        let (answer, findings) = resolve_findings(reply, &top, &chunks, &files);
        assert!(answer.contains("96,307"));
        assert_eq!(findings.len(), 2, "range-invalid citations dropped");
        assert_eq!(findings[0]["file"], "fin/accounts.txt");
        assert_eq!(findings[0]["page"], 1);
        assert_eq!(findings[0]["verified"], true);
        assert_eq!(findings[1]["verified"], false, "paraphrase flagged, not trusted");
        // Non-JSON reply degrades to prose with zero findings.
        let (answer, findings) = resolve_findings("I could not find it.", &top, &chunks, &files);
        assert_eq!(answer, "I could not find it.");
        assert!(findings.is_empty());
    }

    /// LIVE smoke of the leg unit tests cannot reach: the real prompt against
    /// the real model -> JSON -> excerpt resolution -> verbatim verification.
    /// Run: cargo test --lib data_room_live_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn data_room_live_smoke() {
        let Some(key) = crate::commands::secrets::get_api_key() else {
            eprintln!("no OpenRouter key in the credential store; skipping");
            return;
        };
        let (files, chunks) = room(&[
            (
                "fin/accounts.txt",
                "Project Atlas - audited accounts.\n\nRevenue for fiscal year 2025 was 96,307 thousand USD, up from 80,120 thousand USD in fiscal 2024. EBITDA margin held at 31 percent.",
            ),
            ("legal/nda.txt", "Mutual non-disclosure agreement between the parties, governed by Delaware law."),
        ]);
        let question = "What was FY2025 revenue?";
        let top = bm25_top_k(&chunks, question, TOP_K);
        assert_eq!(top[0], 0, "retrieval sanity");
        let excerpts = build_excerpts(&top, &chunks, &files);
        let cfg = fm_extract::LlmConfig {
            api_key: key,
            model: "openai/gpt-4.1-mini".into(),
        };
        let content = fm_extract::llm_complete_with(
            Some(&cfg),
            ROOM_PROMPT,
            &format!("Question: {question}\n\nExcerpts:\n{excerpts}"),
            700,
        )
        .expect("live completion");
        eprintln!("MODEL REPLY:\n{content}");
        let (answer, findings) = resolve_findings(&content, &top, &chunks, &files);
        eprintln!("ANSWER: {answer}\nFINDINGS: {findings:?}");
        assert!(!findings.is_empty(), "no findings came back");
        assert_eq!(findings[0]["file"], "fin/accounts.txt");
        assert!(
            findings.iter().any(|f| f["verified"] == true),
            "no verbatim-verified quote"
        );
        assert!(answer.contains("96,307") || answer.to_lowercase().contains("96.3"));
    }

    #[test]
    fn tag_strip_keeps_text_drops_script() {
        let html = "<html><script>evil()</script><body><h1>Revenue</h1><p>was <b>96,307</b></p></body></html>";
        let t = strip_tags(html);
        assert!(t.contains("Revenue"));
        assert!(t.contains("96,307"));
        assert!(!t.contains("evil"));
    }
}
