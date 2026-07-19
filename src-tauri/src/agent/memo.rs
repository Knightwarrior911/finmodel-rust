//! Memo drafting engine — the "write it up" leg of research → numbers → prose.
//!
//! Anti-slop by construction, sized for a small model (gpt-4.1-mini writes
//! the prose in production tests):
//! - The SKELETON is deterministic: title, dated header, metric tables, and
//!   the numbered sources list are composed by the app from the
//!   conversation's evidence cards — never by the model.
//! - The model only fills narrow prose slots (1–3 sentences each), one call
//!   per slot, against an explicit fact pack.
//! - Every slot is VALIDATED: numeric tokens must exist in the evidence
//!   (no invented numbers), banned slop phrasing rejects, length caps hold,
//!   and citation markers must reference real sources. One retry with the
//!   rejection reason; then the slot falls back to a deterministic sentence
//!   composed from the facts — a memo never fails to exist and never lies.

use std::collections::HashSet;

use serde_json::Value;

/// Memo kinds an analyst actually writes. Kept to three until each earns
/// its structure.
pub const KINDS: &[&str] = &["earnings_note", "company_profile", "deal_summary"];

pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        "earnings_note" => "Earnings note",
        "company_profile" => "Company profile",
        "deal_summary" => "Deal summary",
        _ => "Memo",
    }
}

/// Evidence distilled from the conversation's result cards.
#[derive(Debug, Default)]
pub struct Evidence {
    /// Human "Label: value" fact lines (financial rows, deal facts, quotes).
    pub facts: Vec<String>,
    /// Cited research prose (already citation-validated upstream).
    pub notes: Vec<String>,
    /// (title-ish, url) pairs, first-seen order → memo source numbering.
    pub sources: Vec<(String, String)>,
    /// Normalized numeric tokens that prose is allowed to use.
    pub numbers: HashSet<String>,
    /// Company display name / ticker, best-effort.
    pub company: String,
}

impl Evidence {
    pub fn is_empty(&self) -> bool {
        self.facts.is_empty() && self.notes.is_empty()
    }
}

/// Normalize a numeric token for whitelist membership: strip formatting that
/// prose legitimately varies ("$97,690M" == "97690").
pub fn normalize_num(tok: &str) -> String {
    let mut s: String = tok
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    // Canonical decimal: "82056.0" == "82056", "7,091" == "7091".
    if s.contains('.') {
        s = s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    s.trim_start_matches('0').to_string()
}

/// Numeric tokens in a text (digit runs incl. decimals/commas).
pub fn extract_numbers(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() || ((c == '.' || c == ',') && !cur.is_empty()) {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out.into_iter()
        .map(|t| t.trim_matches(|c| c == '.' || c == ',').to_string())
        .filter(|t| !t.is_empty())
        .collect()
}

/// A 4-digit year is exempt from the whitelist (periods are named freely).
fn is_year(tok: &str) -> bool {
    tok.len() == 4
        && (tok.starts_with("19") || tok.starts_with("20"))
        && tok.chars().all(|c| c.is_ascii_digit())
}

/// Register every number in `text` into the evidence whitelist.
fn absorb_numbers(numbers: &mut HashSet<String>, text: &str) {
    for t in extract_numbers(text) {
        let n = normalize_num(&t);
        if n.is_empty() {
            continue;
        }
        // Analysts round: 97,690M becomes "$97.7 billion" in prose. Derive
        // the legitimate roundings deterministically so natural phrasing
        // passes while invented figures still reject.
        if let Ok(v) = n.parse::<f64>() {
            if v >= 1000.0 {
                let b = v / 1000.0;
                numbers.insert(normalize_num(&format!("{b:.2}")));
                numbers.insert(normalize_num(&format!("{b:.1}")));
                numbers.insert(normalize_num(&format!("{:.0}", v)));
            }
            if v >= 10.0 {
                numbers.insert(normalize_num(&format!("{v:.1}")));
                numbers.insert(normalize_num(&format!("{v:.0}")));
            }
        }
        numbers.insert(n);
    }
}

/// Distill the conversation's result cards into an evidence pack.
pub fn collect_evidence(cards: &[Value]) -> Evidence {
    let mut ev = Evidence::default();
    for card in cards {
        match card["type"].as_str().unwrap_or("") {
            "financials" => {
                if ev.company.is_empty() {
                    ev.company = card["entity"]
                        .as_str()
                        .or_else(|| card["ticker"].as_str())
                        .unwrap_or("")
                        .to_string();
                }
                let periods: Vec<String> = card["periods"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|p| p["label"].as_str().unwrap_or("").to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                for row in card["rows"].as_array().unwrap_or(&Vec::new()) {
                    let label = row["label"].as_str().unwrap_or("");
                    if let Some(vals) = row["values"].as_array() {
                        let cells: Vec<String> = vals
                            .iter()
                            .enumerate()
                            .map(|(i, v)| {
                                let p = periods.get(i).cloned().unwrap_or_default();
                                format!("{p}: {}", v.as_str().unwrap_or("—"))
                            })
                            .collect();
                        let line = format!("{label} — {}", cells.join("; "));
                        absorb_numbers(&mut ev.numbers, &line);
                        ev.facts.push(line);
                    } else {
                        let disp = row["display"]
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| row["value"].to_string());
                        let line = format!("{label}: {disp}");
                        absorb_numbers(&mut ev.numbers, &line);
                        ev.facts.push(line);
                    }
                }
                for seg in card["segments"].as_array().unwrap_or(&Vec::new()) {
                    let line = format!(
                        "Segment {}: {:.1}M ({})",
                        seg["segment"].as_str().unwrap_or(""),
                        seg["value"].as_f64().unwrap_or(0.0) / 1.0e6,
                        seg["period_end"].as_str().unwrap_or("")
                    );
                    absorb_numbers(&mut ev.numbers, &line);
                    ev.facts.push(line);
                }
                if let Some(u) = card["source"].as_str() {
                    ev.sources.push(("SEC EDGAR company facts".into(), u.into()));
                }
            }
            "quote" => {
                let line = format!(
                    "Market quote {}: {} {}",
                    card["ticker"].as_str().unwrap_or(""),
                    card["price"].as_f64().unwrap_or(0.0),
                    card["currency"].as_str().unwrap_or("")
                );
                absorb_numbers(&mut ev.numbers, &line);
                ev.facts.push(line);
            }
            "research_answer" => {
                let a = &card["answer"];
                if let Some(s) = a["summary"]["text"].as_str() {
                    absorb_numbers(&mut ev.numbers, s);
                    ev.notes.push(s.to_string());
                }
                for sec in a["sections"].as_array().unwrap_or(&Vec::new()) {
                    for p in sec["paragraphs"].as_array().unwrap_or(&Vec::new()) {
                        if let Some(t) = p["text"].as_str() {
                            absorb_numbers(&mut ev.numbers, t);
                            ev.notes.push(t.to_string());
                        }
                    }
                }
                for s in a["sources"].as_array().unwrap_or(&Vec::new()) {
                    let url = s["final_url"]
                        .as_str()
                        .or_else(|| s["requested_url"].as_str())
                        .unwrap_or("");
                    if !url.is_empty() {
                        let title = s["title"].as_str().unwrap_or(url).to_string();
                        ev.sources.push((title, url.to_string()));
                    }
                }
            }
            "deal" => {
                if let Some(sum) = card["summary"].as_object() {
                    for (k, v) in sum {
                        if v.is_null() {
                            continue;
                        }
                        let line = format!("{k}: {v}");
                        absorb_numbers(&mut ev.numbers, &line);
                        ev.facts.push(line);
                    }
                }
                for u in card["sources_read"].as_array().unwrap_or(&Vec::new()) {
                    if let Some(u) = u.as_str() {
                        ev.sources.push((u.to_string(), u.to_string()));
                    }
                }
            }
            "filing_doc" => {
                if let Some(p) = card["preview"].as_str() {
                    absorb_numbers(&mut ev.numbers, p);
                    ev.notes.push(p.to_string());
                }
                if let Some(u) = card["url"].as_str() {
                    let bits = [
                        card["ticker"].as_str().unwrap_or(""),
                        card["form"].as_str().unwrap_or(""),
                        card["filing_date"].as_str().unwrap_or(""),
                    ]
                    .iter()
                    .filter(|s| !s.is_empty())
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ");
                    ev.sources.push((bits, u.to_string()));
                }
            }
            "model" => {
                if ev.company.is_empty() {
                    ev.company = card["ticker"].as_str().unwrap_or("").to_string();
                }
                let cur = card["currency"].as_str().unwrap_or("USD");
                let v = &card["valuation"];
                if let Some(pps) = v["price_per_share"].as_f64() {
                    let line = format!(
                        "DCF value per share: {pps:.2} {cur} ({} case, {})",
                        card["case"].as_str().unwrap_or("base"),
                        v["method"].as_str().unwrap_or("DCF")
                    );
                    absorb_numbers(&mut ev.numbers, &line);
                    ev.facts.push(line);
                }
                if let Some(px) = v["current_price"].as_f64() {
                    let line = format!("Current share price: {px:.2} {cur}");
                    absorb_numbers(&mut ev.numbers, &line);
                    ev.facts.push(line);
                }
                if let Some(u) = v["upside_pct"].as_f64() {
                    let line = format!("Implied upside to DCF: {:.1}%", u * 100.0);
                    absorb_numbers(&mut ev.numbers, &line);
                    ev.facts.push(line);
                }
                if let Some(evv) = v["ev"].as_f64() {
                    let line = format!("Enterprise value: {:.0}M {cur}", evv / 1.0e6);
                    absorb_numbers(&mut ev.numbers, &line);
                    ev.facts.push(line);
                }
                if let Some(w) = v["wacc"].as_f64() {
                    let line = format!("WACC: {:.2}%", w * 100.0);
                    absorb_numbers(&mut ev.numbers, &line);
                    ev.facts.push(line);
                }
            }
            "benchmark" => {
                // Peer comps table: one dense fact line per peer row, using
                // the card's own header labels so memo prose matches the UI.
                let headers: Vec<(String, String)> = card["headers"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(|h| {
                                (
                                    h["key"].as_str().unwrap_or("").to_string(),
                                    h["label"].as_str().unwrap_or("").to_string(),
                                )
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                for row in card["rows"].as_array().unwrap_or(&Vec::new()) {
                    let mut bits: Vec<String> = Vec::new();
                    for (key, label) in &headers {
                        let val = &row[key.as_str()];
                        if val.is_null() || key == "ticker" {
                            continue;
                        }
                        let shown = val
                            .as_str()
                            .map(str::to_string)
                            .unwrap_or_else(|| val.to_string());
                        bits.push(format!("{label} {shown}"));
                    }
                    if bits.is_empty() {
                        continue;
                    }
                    let line = format!(
                        "Peer {}: {}",
                        row["ticker"].as_str().unwrap_or("?"),
                        bits.join(", ")
                    );
                    absorb_numbers(&mut ev.numbers, &line);
                    ev.facts.push(line);
                }
            }
            _ => {}
        }
    }
    // Dedupe sources by url, keep first-seen order.
    let mut seen = HashSet::new();
    ev.sources.retain(|(_, u)| seen.insert(u.clone()));
    ev
}

/// Prose slots per memo kind: (heading, writing instruction, max sentences).
pub fn section_specs(kind: &str) -> Vec<(&'static str, &'static str, usize)> {
    match kind {
        "earnings_note" => vec![
            ("Headline", "One sentence stating the single most important takeaway of the period, led by the number that proves it.", 1),
            ("Results", "Two to three sentences on revenue, profitability, and the standout line items versus the prior period. Numbers only from the fact pack.", 3),
            ("Drivers and outlook", "Two to three sentences on what drove the quarter and any guidance or forward commentary found in the research notes. If no guidance appears in the evidence, say management gave none in the sources reviewed.", 3),
        ],
        "company_profile" => vec![
            ("Business", "Two to three sentences describing what the company actually sells and to whom, from the research notes only.", 3),
            ("Financial position", "Two to three sentences on scale and trajectory: revenue, margins, cash generation — numbers only from the fact pack.", 3),
            ("Considerations", "Two sentences on the material watch-items an analyst would flag from the evidence (competition, concentration, regulatory).", 2),
        ],
        "deal_summary" => vec![
            ("Transaction", "One to two sentences: who is acquiring whom, headline value, and consideration structure, from the facts.", 2),
            ("Strategic rationale", "Two sentences on why, per the parties' own statements in the research notes.", 2),
            ("Economics", "Two sentences on price, synergies, and financing where stated in the evidence. Never estimate what the sources do not state.", 2),
        ],
        _ => vec![],
    }
}

/// Slop phrasing that instantly rejects a slot: hedge-bot filler and
/// adjective inflation no analyst would sign.
const SLOP: &[&str] = &[
    "as an ai",
    "i cannot",
    "i'm unable",
    "it's important to note",
    "it is important to note",
    "in conclusion",
    "furthermore",
    "delve",
    "landscape",
    "impressive",
    "remarkable",
    "poised to",
    "testament to",
    "game-changer",
    "cutting-edge",
];

/// Validate one prose slot against the evidence. Errors carry the reason so a
/// retry prompt can quote it.
pub fn validate_slot(text: &str, ev: &Evidence, max_sentences: usize) -> Result<(), String> {
    let t = text.trim();
    if t.is_empty() {
        return Err("empty".into());
    }
    if t.len() > 700 {
        return Err("too long".into());
    }
    // Sentence terminators only when followed by whitespace/end — a decimal
    // point in "$97.7 billion" is not a sentence break (a live mini draft
    // was wrongly rejected for exactly this).
    let chars: Vec<char> = t.chars().collect();
    let mut sentences = 0usize;
    for (i, c) in chars.iter().enumerate() {
        if matches!(c, '.' | '!' | '?') {
            let next_ws = chars.get(i + 1).map(|n| n.is_whitespace()).unwrap_or(true);
            if next_ws {
                sentences += 1;
            }
        }
    }
    let sentences = sentences.max(1);
    if sentences > max_sentences + 1 {
        return Err(format!("too many sentences ({sentences} > {max_sentences})"));
    }
    let lower = t.to_lowercase();
    for s in SLOP {
        if lower.contains(s) {
            return Err(format!("banned phrasing: \"{s}\""));
        }
    }
    // Citation markers must reference real sources.
    let mut rest = t;
    while let Some(i) = rest.find("[S") {
        let tail = &rest[i + 2..];
        let id: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(n) = id.parse::<usize>() {
            if n == 0 || n > ev.sources.len() {
                return Err(format!("citation [S{n}] has no matching source"));
            }
        }
        rest = &rest[i + 2..];
    }
    // Every non-year number must exist in the evidence.
    for tok in extract_numbers(t) {
        if is_year(&tok) {
            continue;
        }
        let n = normalize_num(&tok);
        if n.is_empty() || n.len() < 2 {
            continue; // single digits: list markers, "3 segments", etc.
        }
        if !ev.numbers.contains(&n) {
            return Err(format!("number {tok} is not in the evidence"));
        }
    }
    Ok(())
}

/// Deterministic fallback prose for a slot: honest fact lines, no synthesis.
pub fn fallback_text(heading: &str, ev: &Evidence) -> String {
    let mut lines: Vec<&str> = Vec::new();
    let wants_prose = matches!(
        heading,
        "Business" | "Strategic rationale" | "Drivers and outlook" | "Considerations"
    );
    if wants_prose && !ev.notes.is_empty() {
        for n in ev.notes.iter().take(2) {
            lines.push(n);
        }
    } else {
        for f in ev.facts.iter().take(3) {
            lines.push(f);
        }
    }
    if lines.is_empty() {
        return "The sources reviewed did not support a statement for this section.".into();
    }
    lines
        .iter()
        .map(|l| l.trim_end_matches('.').to_string() + ".")
        .collect::<Vec<_>>()
        .join(" ")
}

/// Render the full memo: deterministic scaffold + validated prose slots.
pub fn render_markdown(
    kind: &str,
    company: &str,
    date: &str,
    sections: &[(String, String, bool)],
    ev: &Evidence,
) -> String {
    let mut md = String::new();
    md.push_str(&format!("# {} — {}\n\n", company, kind_label(kind)));
    md.push_str(&format!("*Prepared {date} · finmodel · sources cited below*\n\n"));
    for (heading, text, fell_back) in sections {
        md.push_str(&format!("## {heading}\n\n{text}\n"));
        if *fell_back {
            md.push_str("\n*(Composed directly from the evidence — the drafting model's text did not pass validation.)*\n");
        }
        md.push('\n');
    }
    if !ev.facts.is_empty() {
        md.push_str("## Key figures\n\n");
        md.push_str("| Item | Value |\n|---|---|\n");
        for f in ev.facts.iter().take(20) {
            let (k, v) = f.split_once([':', '—']).unwrap_or((f.as_str(), ""));
            md.push_str(&format!("| {} | {} |\n", k.trim(), v.trim().replace('|', "/")));
        }
        md.push('\n');
    }
    if !ev.sources.is_empty() {
        md.push_str("## Sources\n\n");
        for (i, (title, url)) in ev.sources.iter().enumerate() {
            md.push_str(&format!("{}. {} — {}\n", i + 1, title, url));
        }
        md.push('\n');
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn pack() -> Evidence {
        collect_evidence(&[
            json!({
                "type": "financials", "entity": "Tesla, Inc.", "ticker": "TSLA",
                "source": "https://www.sec.gov/cgi-bin/browse-edgar?CIK=TSLA",
                "rows": [
                    {"label": "Revenue", "display": "97,690M"},
                    {"label": "Net income", "display": "7,091M"}
                ],
                "segments": [
                    {"segment": "Automotive", "value": 82056000000.0, "period_end": "2025-12-31", "eliminations": false}
                ]
            }),
            json!({
                "type": "research_answer",
                "answer": {
                    "summary": {"text": "Management flagged tariff pressure on margins [S1]."},
                    "sections": [],
                    "sources": [
                        {"id": "S1", "final_url": "https://ir.tesla.com/press/q1", "title": "Q1 Update"}
                    ]
                }
            })
        ])
    }

    #[test]
    fn evidence_collects_facts_numbers_and_sources() {
        let ev = pack();
        assert_eq!(ev.company, "Tesla, Inc.");
        assert!(ev.facts.iter().any(|f| f.contains("Revenue")));
        assert!(ev.numbers.contains("97690"));
        assert!(ev.numbers.contains("7091"));
        assert!(ev.numbers.contains("82056")); // segment in millions
        assert_eq!(ev.sources.len(), 2);
        assert!(!ev.is_empty());
    }

    #[test]
    fn validation_rejects_invented_numbers_and_accepts_evidence_numbers() {
        let ev = pack();
        // Evidence numbers, formatted differently, pass.
        assert!(validate_slot("Revenue reached $97,690M with net income of 7091M.", &ev, 2).is_ok());
        // An invented figure rejects with the offending token named.
        let err = validate_slot("Revenue reached $99,999M.", &ev, 2).unwrap_err();
        assert!(err.contains("99,999"), "{err}");
        // Years are exempt; small counts are exempt.
        assert!(validate_slot("In 2025 the 2 segments both grew.", &ev, 2).is_ok());
    }

    #[test]
    fn validation_rejects_slop_length_and_phantom_citations() {
        let ev = pack();
        assert!(validate_slot("An impressive quarter.", &ev, 2).is_err());
        assert!(validate_slot("It's important to note growth.", &ev, 2).is_err());
        let many = "Revenue grew. ".repeat(9);
        assert!(validate_slot(&many, &ev, 2).is_err());
        let err = validate_slot("Margins fell [S7].", &ev, 2).unwrap_err();
        assert!(err.contains("[S7]"), "{err}");
        assert!(validate_slot("Margins compressed on tariffs [S1].", &ev, 2).is_ok());
    }

    #[test]
    fn fallback_is_honest_and_nonempty() {
        let ev = pack();
        let f = fallback_text("Results", &ev);
        assert!(f.contains("Revenue"), "{f}");
        let d = fallback_text("Drivers and outlook", &ev);
        assert!(d.contains("tariff"), "{d}");
        let empty = fallback_text("Results", &Evidence::default());
        assert!(empty.contains("did not support"));
    }

    #[test]
    fn markdown_scaffold_is_complete() {
        let ev = pack();
        let md = render_markdown(
            "earnings_note",
            "Tesla, Inc.",
            "2026-07-19",
            &[
                ("Headline".into(), "Revenue reached 97,690M.".into(), false),
                ("Results".into(), "Net income was 7,091M.".into(), true),
            ],
            &ev,
        );
        assert!(md.starts_with("# Tesla, Inc. — Earnings note"));
        assert!(md.contains("## Headline"));
        assert!(md.contains("did not pass validation"));
        assert!(md.contains("## Key figures"));
        assert!(md.contains("| Revenue |"));
        assert!(md.contains("## Sources"));
        assert!(md.contains("1. "));
    }

    #[test]
    fn every_kind_has_specs_and_labels() {
        for k in KINDS {
            assert!(!section_specs(k).is_empty(), "{k}");
            assert_ne!(kind_label(k), "Memo", "{k}");
        }
        assert!(section_specs("nonsense").is_empty());
    }

    /// REAL card shapes: fixtures captured from a live user database (the
    /// exact JSON the Rust card builders emit) — field-shape drift between
    /// builders and this consumer fails HERE, not in front of the user.
    #[test]
    fn real_app_cards_distill() {
        let raw = include_str!("../../tests/fixtures/real_cards.json");
        let fixtures: serde_json::Value = serde_json::from_str(raw).unwrap();
        let cards: Vec<Value> = fixtures
            .as_object()
            .unwrap()
            .values()
            .cloned()
            .collect();
        let ev = collect_evidence(&cards);
        assert!(!ev.is_empty(), "real cards produced no evidence");
        assert!(!ev.facts.is_empty(), "no facts from real financials/quote cards");
        assert!(!ev.numbers.is_empty(), "no numbers absorbed");
        assert!(!ev.sources.is_empty(), "no sources from real research card");
        assert!(!ev.company.is_empty(), "company not inferred from real card");
        // The real model card (TSLA DCF) distills into valuation facts.
        assert!(
            ev.facts.iter().any(|f| f.starts_with("DCF value per share")),
            "model card valuation not distilled"
        );
        assert!(ev.numbers.contains("32.36"), "DCF per-share number missing");
        // The scaffold renders from real evidence without panicking.
        let md = render_markdown("earnings_note", &ev.company.clone(), "2026-07-19", &[], &ev);
        assert!(md.contains("## Key figures"));
        assert!(md.contains("## Sources"));
    }

    #[test]
    fn benchmark_card_distills_peer_rows() {
        // Exact shape from tool_benchmark in commands/chat.rs.
        let card = json!({
            "type": "benchmark",
            "title": "NVDA vs AMD comps",
            "headers": [
                { "key": "ticker", "label": "Ticker" },
                { "key": "fiscal_year", "label": "FY" },
                { "key": "revenue_m", "label": "Revenue (m)" },
                { "key": "ebitda_margin", "label": "EBITDA margin" }
            ],
            "rows": [
                { "ticker": "NVDA", "fiscal_year": "2025", "revenue_m": 130497.0, "ebitda_margin": 0.64 },
                { "ticker": "AMD", "fiscal_year": "2024", "revenue_m": 25785.0, "ebitda_margin": null }
            ]
        });
        let ev = collect_evidence(&[card]);
        assert_eq!(ev.facts.len(), 2);
        assert!(ev.facts[0].starts_with("Peer NVDA: FY 2025, Revenue (m) 130497"));
        assert!(ev.facts[0].contains("EBITDA margin 0.64"));
        // Null cells are dropped, not rendered as null.
        assert!(!ev.facts[1].contains("null"));
        assert!(ev.numbers.contains("130497"));
    }

    /// LIVE (network + configured key): one real slot written by the
    /// PRODUCTION TEST MODEL (gpt-4.1-mini per settings) through the exact
    /// write→validate→retry loop. Proves the mini model can pass the
    /// discipline — or that the fallback engages cleanly.
    /// Run: cargo test --lib live_memo_slot_mini -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_memo_slot_mini() {
        let Some(key) = crate::commands::secrets::get_api_key() else {
            panic!("no API key in the credential store");
        };
        let ev = pack();
        let (heading, instruction, max_s) = section_specs("earnings_note")[0];
        let system = "You are drafting one section of an investment-banking memo. Use ONLY the facts provided. Cite research statements as [S<n>] where applicable. No hedging filler, no adjectives like impressive/remarkable. Plain declarative sentences.";
        let user = format!(
            "Section: {heading}\nInstruction: {instruction}\nMax sentences: {max_s}\n\nFACTS:\n{}\n\nRESEARCH NOTES:\n{}\n\nSOURCES:\n{}",
            ev.facts.join("\n"),
            ev.notes.join("\n"),
            ev.sources.iter().enumerate().map(|(i, (t, _))| format!("[S{}] {}", i + 1, t)).collect::<Vec<_>>().join("\n"),
        );
        let out = crate::commands::settings::complete_once(
            &key,
            "openai/gpt-4.1-mini",
            "https://openrouter.ai/api/v1/chat/completions",
            system,
            &user,
            300,
        )
        .expect("mini call");
        println!("mini wrote: {out}");
        match validate_slot(&out, &ev, max_s) {
            Ok(()) => println!("VALIDATED on first pass"),
            Err(e) => {
                println!("rejected ({e}); retrying with reason…");
                let retry = crate::commands::settings::complete_once(
                    &key,
                    "openai/gpt-4.1-mini",
                    "https://openrouter.ai/api/v1/chat/completions",
                    system,
                    &format!("{user}\n\nYour previous draft was REJECTED: {e}. Fix exactly that and rewrite."),
                    300,
                )
                .expect("mini retry");
                println!("mini rewrote: {retry}");
                match validate_slot(&retry, &ev, max_s) {
                    Ok(()) => println!("VALIDATED on retry"),
                    Err(e2) => println!("fallback engages ({e2}): {}", fallback_text(heading, &ev)),
                }
            }
        }
    }
}
