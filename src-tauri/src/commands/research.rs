//! Async research driver (Phase 2.2 / 3.1).
//!
//! `run_research` is the single async action driver: it pumps the pure
//! [`fm_research::ResearchMachine`] reducer, executing each emitted action
//! against a [`ResearchBackend`] (search/read via `fm_fetch`/`fm_mcp`) and a
//! [`ResearchSynthesizer`] (the bounded weak-model call + validation), and feeds
//! the typed results back. The reducer owns all policy (stage order, budgets,
//! one-repair-then-digest, cancellation); this driver only performs I/O and
//! wiring. Because the adapters are traits, the whole pump is unit-tested here
//! with fakes — the real network adapters wire in without changing the pump.

use fm_research::machine::{Action, Input, ResearchMachine, SynthesisReject};
use fm_research::research::{
    ResearchAnswer, ResearchMode, ResearchPlan, ResearchRequest, SourceRecord,
};
use fm_research::synth::{build_answer, validate_synthesis, SynthReject};
use serde_json::{json, Value};

use std::future::Future;
use std::time::Duration;
/// Search/read backend the driver executes (real impl maps `fm_fetch`/`fm_mcp`).
#[allow(async_fn_in_trait)]
pub trait ResearchBackend {
    /// Ask the model for a bounded plan (Standard/Deep). `None` = fall back to
    /// the unchanged question.
    async fn plan(&self, request: &ResearchRequest) -> Option<ResearchPlan>;
    /// Run the queries; return ranked, deduped, budget-capped candidate records
    /// with stable `S#` ids assigned (real impl uses `fm_research::assemble_ledger`).
    async fn search(&self, queries: &[String]) -> Vec<SourceRecord>;
    /// Read the candidate ledger, returning records with final statuses/excerpts.
    async fn read(&self, ledger: Vec<SourceRecord>) -> Vec<SourceRecord>;
}

/// The bounded synthesis step: validate the model's draft against the read
/// ledger and build the trusted answer, or reject for one repair.
#[allow(async_fn_in_trait)]
pub trait ResearchSynthesizer {
    async fn synthesize(
        &self,
        attempt: u32,
        read: &[SourceRecord],
    ) -> Result<ResearchAnswer, SynthReject>;
}

/// A backend seeded with a stored read ledger (Phase 3.6 retry). Returns the
/// stored sources from `search` (already `Read`) and passes them through
/// `read`, so the driver SKIPS network search/read and re-runs only synthesis.
pub struct SeededBackend {
    pub ledger: Vec<SourceRecord>,
}

impl ResearchBackend for SeededBackend {
    async fn plan(&self, _request: &ResearchRequest) -> Option<ResearchPlan> {
        None
    }
    async fn search(&self, _queries: &[String]) -> Vec<SourceRecord> {
        self.ledger.clone()
    }
    async fn read(&self, ledger: Vec<SourceRecord>) -> Vec<SourceRecord> {
        ledger
    }
}

/// Drive a research run to a terminal [`Action`] (`Done`/`Cancelled`/`Error`).
///
/// The reducer decides everything; the driver just executes I/O for each action
/// and feeds results back. Cancellation is delivered by the reducer receiving a
/// `Cancel` input when `cancel` is set. The overall depth deadline is a real
/// wall-clock bound: each backend/synth await is wrapped in
/// `tokio::time::timeout(remaining)`; on elapse the driver injects `Deadline`
/// so the reducer can emit an honest partial digest.
pub async fn run_research<B, S>(
    mut machine: ResearchMachine,
    request: ResearchRequest,
    backend: &B,
    synth: &S,
    cancel: &tokio_util::sync::CancellationToken,
    progress: &dyn Fn(&str),
) -> Action
where
    B: ResearchBackend,
    S: ResearchSynthesizer,
{
    use std::time::{Duration, Instant};
    let mut input = Input::Start;
    // The driver caches the searched ledger (for reads) and the read ledger (for
    // synthesis); the reducer tracks its own copy for policy decisions.
    let mut searched: Vec<SourceRecord> = Vec::new();
    let mut read_records: Vec<SourceRecord> = Vec::new();
    let started = Instant::now();
    let deadline = Duration::from_secs(request.depth.budgets().deadline_secs.max(1));
    loop {
        // Pre-check so a zero-remaining budget never starts another stage.
        if cancel.is_cancelled() {
            input = Input::Cancel;
        } else if started.elapsed() >= deadline {
            input = Input::Deadline;
        }
        let action = machine.next(input);
        match action {
            Action::Done(_) | Action::Cancelled | Action::Error { .. } => return action,
            Action::Plan => {
                progress("Planning searches");
                let rem = deadline.saturating_sub(started.elapsed());
                input = race_stage(cancel, rem, backend.plan(&request), Input::Planned).await;
            }
            Action::Search { queries } => {
                progress(&format!("Searching {} queries", queries.len().max(1)));
                let rem = deadline.saturating_sub(started.elapsed());
                input = race_stage(cancel, rem, backend.search(&queries), |ledger| {
                    searched = ledger;
                    Input::Searched(searched.clone())
                })
                .await;
            }
            Action::Read { .. } => {
                progress(&format!("Reading {} sources", searched.len().max(1)));
                let rem = deadline.saturating_sub(started.elapsed());
                input = race_stage(cancel, rem, backend.read(searched.clone()), |ledger| {
                    read_records = ledger;
                    Input::ReadDone(read_records.clone())
                })
                .await;
            }
            Action::Synthesize { attempt } => {
                progress("Synthesizing answer");
                let rem = deadline.saturating_sub(started.elapsed());
                input = race_stage(
                    cancel,
                    rem,
                    synth.synthesize(attempt, &read_records),
                    |result| {
                        Input::Synthesized(result.map_err(|e| SynthesisReject {
                            code: e.code().to_string(),
                        }))
                    },
                )
                .await;
            }
        }
    }
}

/// Race a stage future against cancel + remaining wall-clock so Stop is
/// mid-stage, not only between stages.
async fn race_stage<T, F, Map>(
    cancel: &tokio_util::sync::CancellationToken,
    rem: Duration,
    fut: F,
    map: Map,
) -> Input
where
    F: Future<Output = T>,
    Map: FnOnce(T) -> Input,
{
    tokio::select! {
        _ = cancel.cancelled() => Input::Cancel,
        res = tokio::time::timeout(rem, fut) => match res {
            Ok(v) => map(v),
            Err(_) => Input::Deadline,
        }
    }
}

// ── Real adapters: OpenRouter synthesizer ───────────────────────────────────

const OPENROUTER_CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// OpenRouter-backed synthesizer: prompts for a cited draft, validates it
/// against the read ledger (every citation resolves to a `Read` source with an
/// exact quote), and builds the trusted answer. Infrastructure/parse failures
/// map to a rejection so the reducer repairs once, then emits an honest digest.
pub struct OpenRouterSynthesizer {
    pub api_key: String,
    pub model: String,
    pub strict_json: bool,
    pub request: ResearchRequest,
}

impl ResearchSynthesizer for OpenRouterSynthesizer {
    async fn synthesize(
        &self,
        _attempt: u32,
        read: &[SourceRecord],
    ) -> Result<ResearchAnswer, SynthReject> {
        use fm_research::research::SourceStatus;
        let reads: Vec<&SourceRecord> = read
            .iter()
            .filter(|r| r.status == SourceStatus::Read)
            .collect();
        if reads.is_empty() {
            return Err(SynthReject::Empty);
        }
        let (system, user) = fm_research::synth::synthesis_prompt(&self.request, read);
        let mut body = json!({
            "model": self.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user },
            ],
            "temperature": 0,
            "stream": false,
            // Ample budget so a reasoning model's tokens don't truncate the JSON.
            "max_tokens": 8000,
        });
        if self.strict_json {
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": "research_synthesis", "strict": true, "schema": fm_research::synth::synthesis_schema() }
            });
            body["provider"] = json!({ "require_parameters": true });
        }
        // Reuse the application-lived async client (Phase 3.3).
        let client = crate::commands::chat::shared_http_client()
            .cloned()
            .unwrap_or_else(|_| reqwest::Client::new());
        let resp = client
            .post(OPENROUTER_CHAT_URL)
            .bearer_auth(&self.api_key)
            .header("HTTP-Referer", "https://github.com/finmodel")
            .header("X-Title", "finmodel")
            .json(&body)
            .send()
            .await
            .map_err(|_| SynthReject::Empty)?;
        if !resp.status().is_success() {
            return Err(SynthReject::Empty);
        }
        let v: Value = resp.json().await.map_err(|_| SynthReject::Empty)?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or_default();
        let draft = fm_research::synth::parse_draft(content).ok_or(SynthReject::Empty)?;
        validate_synthesis(&draft, read)?;
        Ok(build_answer(
            &draft,
            &self.request,
            read.to_vec(),
            &self.model,
            fm_research::today_iso(),
        ))
    }
}

// ── Real adapter: BasicHttp backend (DuckDuckGo search + safe page read) ─────

use std::net::ToSocketAddrs;

/// True when the URL's path names a PDF (query/fragment ignored). IR decks,
/// annual reports, and investor presentations — the exact sources the
/// primary-first doctrine hunts — are overwhelmingly PDFs, and the HTML
/// tag-stripper turns them into garbage.
/// Candidate company-name tokens from the research question: lowercase
/// alphabetic words ≥4 chars that are not finance stopwords. Used to spot the
/// company's own website in search hits (see
/// [`fm_research::upgrade_company_candidates`]).
fn question_name_tokens(question: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "about", "annual", "call", "china", "company", "comparison",
        "competition", "earnings", "english", "fashion", "financial",
        "goods", "growth", "guidance", "impact", "income", "interim",
        "investor", "latest", "leather", "management", "margin", "press",
        "presentation", "quarter", "quarterly", "relations", "release",
        "report", "results", "revenue", "said", "sales", "statement",
        "tariff", "tariffs", "transcript", "what", "operating",
    ];
    question
        .split(|c: char| !c.is_ascii_alphabetic())
        .map(|w| w.to_ascii_lowercase())
        .filter(|w| w.len() >= 4 && !STOP.contains(&w.as_str()))
        .collect()
}

/// User-pasted URLs in the research question become PINNED candidates —
/// the user said "use this as the source of truth", so it outranks every
/// heuristic tier (kind still classified honestly for the label). Capped at
/// three; trailing punctuation stripped; banned domains still excluded by
/// the ledger assembler.
fn pinned_candidates(question: &str) -> Vec<fm_research::Candidate> {
    question
        .split_whitespace()
        .filter(|w| w.starts_with("http://") || w.starts_with("https://"))
        .map(|w| w.trim_end_matches(['.', ',', ')', ']', ';', '\'', '"']))
        .filter(|w| url::Url::parse(w).is_ok())
        .take(3)
        .map(|u| fm_research::Candidate {
            pinned: true,
            url: u.to_string(),
            title: String::new(),
            kind: fm_research::classify_source_kind(u),
            backend: fm_research::SourceBackend::BasicHttp,
            snippet: None,
        })
        .collect()
}

fn is_pdf_url(url: &str) -> bool {
    url::Url::parse(url.trim())
        .map(|u| u.path().to_ascii_lowercase().ends_with(".pdf"))
        .unwrap_or(false)
}

/// Download a PDF (25 MB cap, 30 s timeout), extract its text natively via
/// fm-extract (pure Rust — no Python), and window it to the most
/// question-relevant excerpt like the filing reader does. Returns a
/// [`fm_fetch::FetchedPage`] so the standard read-outcome path applies.
fn fetch_pdf_page(url: &str, question: &str) -> Result<fm_fetch::FetchedPage, String> {
    const MAX_PDF_BYTES: usize = 25 * 1024 * 1024;
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .user_agent("finmodel-research/1.0")
        .build()
        .map_err(|e| format!("client:{e}"))?;
    let resp = client.get(url).send().map_err(|e| format!("get:{e}"))?;
    let status = resp.status().as_u16();
    if !(200..300).contains(&status) {
        return Err(format!("http_{status}"));
    }
    let bytes = resp.bytes().map_err(|e| format!("body:{e}"))?;
    if bytes.len() > MAX_PDF_BYTES {
        return Err("pdf_too_large".into());
    }
    if !bytes.starts_with(b"%PDF") {
        return Err("not_a_pdf".into());
    }
    // fm-extract's reader takes a path; use a scoped temp file.
    let tmp = std::env::temp_dir().join(format!(
        "fm-research-{}.pdf",
        fm_agent::ids::format_uuid_v4({
            let mut b = [0u8; 16];
            rand::Rng::fill(&mut rand::thread_rng(), &mut b);
            b
        })
    ));
    std::fs::write(&tmp, &bytes).map_err(|e| format!("tmp:{e}"))?;
    let text = fm_extract::extract_pdf_text(&tmp.to_string_lossy());
    let _ = std::fs::remove_file(&tmp);
    let text = text.map_err(|e| format!("pdf_extract:{e:?}"))?;
    if text.trim().is_empty() {
        // Scanned/image-only PDF — no text layer to cite from.
        return Err("pdf_no_text_layer".into());
    }
    let title = url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|mut s| s.next_back().map(|p| p.to_string()))
        })
        .unwrap_or_default();
    Ok(fm_fetch::FetchedPage {
        title,
        text: fm_research::select_filing_excerpt(&text, question, 4000),
        status: fm_fetch::PageStatus::Ok,
    })
}

/// A UTC `YYYY-MM-DD` retrieval stamp (no date crate).
fn stamp() -> String {
    fm_research::today_iso()
}

/// Resolve `host` and return true if ANY resolved address is a forbidden
/// (loopback/private/link-local/reserved) IP — the pre-connection SSRF guard.
/// Resolution failure returns false so the fetch fails naturally as a read error.
fn host_resolves_to_forbidden(host: &str) -> bool {
    match (host, 443u16).to_socket_addrs() {
        Ok(addrs) => addrs.map(|a| a.ip()).any(fm_research::is_forbidden_ip),
        Err(_) => false,
    }
}

/// The unattended BasicHttp backend: DuckDuckGo search + browser-header page
/// read, mapped into the collector's records with backend/error/final-URL
/// provenance. Blocking `fm_fetch` calls run on the blocking pool so they never
/// stall the async runtime. NOTE: `fm_fetch::fetch_page` currently follows
/// redirects; the per-redirect-hop IP re-validation is the remaining Phase 2.4
/// hardening — the requested URL is validated and DNS-checked here first.
pub struct HttpBackend {
    pub max_sources: u32,
    pub per_query_results: usize,
    /// Research mode — `Filing` routes search/read through EDGAR instead of web.
    pub mode: ResearchMode,
    /// Tickers (filing mode maps each to a CIK).
    pub tickers: Vec<String>,
    /// Requested filing forms (empty → 10-K/10-Q/8-K/20-F/40-F).
    pub filing_forms: Vec<String>,
    /// The question, used to select the most relevant filing items to read.
    pub question: String,
    /// Deal mode: parsed target/acquirer (empty otherwise).
    pub target: String,
    pub acquirer: String,
    /// When present, blocked/unreadable sources get ONE retry through the
    /// user-configured Roam MCP browser — live pages as a human sees them
    /// (bot walls, consent screens, logged-in views), not a cached copy.
    /// A closure, NOT a tauri::AppHandle: tauri types here link the full
    /// windowing runtime into the lib-test binary, whose manifest-less exe
    /// then fails to load (comctl32-v6 TaskDialogIndirect →
    /// STATUS_ENTRYPOINT_NOT_FOUND).
    pub roam: Option<RoamReader>,
}

/// Reads a live page through the user's configured Roam browser. `None` when
/// Roam is unconfigured or the read failed.
pub type RoamReader =
    std::sync::Arc<dyn Fn(&str, &str) -> Option<fm_fetch::FetchedPage> + Send + Sync>;

/// Render a market quote as a compact, citable excerpt with visible freshness.
fn render_quote(q: &fm_fetch::Quote, as_of: &str) -> String {
    let range = match (q.week52_low, q.week52_high) {
        (Some(lo), Some(hi)) => format!(", 52-week range {lo:.2}–{hi:.2}"),
        _ => String::new(),
    };
    format!(
        "{} last price {:.2} {}{}; market data as of {as_of}.",
        q.ticker, q.price, q.currency, range
    )
}

/// Quote a multi-word company name so the engine treats it as one phrase
/// ("Moët Hennessy" ranks the company, not the words). Single tokens and
/// already-quoted text pass through. Works on DuckDuckGo and every MCP
/// backend — quotes are the one universally-portable operator.
fn quote_if_phrase(name: &str) -> String {
    let t = name.trim();
    if t.contains(' ') && !t.starts_with('"') {
        format!("\"{t}\"")
    } else {
        t.to_string()
    }
}

/// Search-operator suffix dropping the HR/noise domains that used to waste
/// result slots (the ledger bans them anyway — better they never arrive).
/// -site: is portable across DuckDuckGo and the MCP engines; date operators
/// (before:/after:) are NOT and never appear here.
const NOISE_EXCLUSIONS: &str = "-site:linkedin.com -site:glassdoor.com -site:indeed.com";

impl HttpBackend {
    /// Company/earnings acquisition: fuse recent filings, web (IR/earnings +
    /// independent), and a market-quote synthetic source into one ranked ledger.
    /// Earnings mode leads with quarterly filings + an earnings-tuned web query.
    async fn fused_search(&self) -> Vec<SourceRecord> {
        let tickers = self.tickers.clone();
        let per_query = self.per_query_results;
        let question = self.question.clone();
        let max_sources = self.max_sources;
        let earnings = self.mode == ResearchMode::Earnings;
        let comparison = self.mode == ResearchMode::Comparison;
        tauri::async_runtime::spawn_blocking(move || {
            let forms: &[&str] = if earnings {
                &["10-Q", "10-K"]
            } else {
                &["10-K", "10-Q"]
            };
            // Comparison takes one filing per ticker; single-company modes take two.
            let filing_limit = if comparison { 1 } else { 2 };
            let mut candidates = Vec::new();
            let mut edgar_hits = 0usize;
            for t in &tickers {
                if let Ok(cik) = fm_fetch::edgar::cik_from_ticker(t) {
                    if let Ok(filings) = fm_fetch::edgar::search_filings(&cik, forms, filing_limit)
                    {
                        edgar_hits += filings.len();
                        candidates.extend(filings.iter().map(fm_research::candidate_from_filing));
                    }
                }
            }
            // Company-authored sources FIRST (IR + earnings/press releases),
            // then the independent web — the analyst's order of evidence.
            // Exchange-suffixed local tickers (MC.PA, 7203.T) are search noise
            // (Bing read "MC.PA LVMH…" as Minecraft). Resolve them to the
            // company's display name via the (cached) quote feed; a question
            // that only says "MC.PA" still becomes an "LVMH …" query.
            let mut subject_parts: Vec<String> = Vec::new();
            let mut question = question.clone();
            for t in &tickers {
                if !t.contains('.') {
                    subject_parts.push(t.clone());
                    continue;
                }
                if let Ok(qt) = fm_fetch::fetch_quote(t) {
                    if let Some(name) = qt.name.filter(|n| !n.trim().is_empty()) {
                        // The raw dotted ticker inside the question is noise
                        // too — swap it for the name everywhere.
                        question = question.replace(t.as_str(), &name);
                        // Multi-word names travel as one quoted phrase.
                        subject_parts.push(quote_if_phrase(&name));
                        continue;
                    }
                }
                // No name available: keep the ticker OUT of the queries.
            }
            let subject = subject_parts.join(" ");
            let core = if subject.is_empty() {
                question.clone()
            } else {
                subject.clone()
            };
            let with_q = |tail: &str| {
                if subject.is_empty() {
                    format!("{question} {tail}")
                } else {
                    format!("{subject} {question} {tail}")
                }
            };
            let mut web_queries: Vec<String> = vec![
                with_q("investor relations"),
                if earnings {
                    format!("{core} earnings release shareholder letter")
                } else {
                    with_q("press release")
                },
            ];
            if earnings {
                // What management SAID lives in the call transcript, not the
                // release. Task-1 PDF ingestion also makes IR transcript PDFs
                // readable when the search lands on one.
                web_queries.push(format!("{core} earnings call transcript"));
            }
            if edgar_hits == 0 {
                // Non-US issuer (or no EDGAR mapping): the filings live on the
                // company site and the local exchange/regulator archive — hunt
                // the primary documents the web way. Large caps keep an
                // English IR mirror; ask for it explicitly (EDINET/TDnet/HKEX
                // era pages are localized). PDF ingestion makes the annual
                // report / presentation results readable.
                web_queries.push(format!("{core} annual report"));
                web_queries.push(format!("{core} investor relations english"));
                web_queries.push(format!("{core} {}", if earnings {
                    "interim results announcement"
                } else {
                    "investor presentation"
                }));
            }
            web_queries.push(if earnings {
                format!("{core} latest quarterly earnings results guidance")
            } else if comparison {
                format!("{} comparison", tickers.join(" vs "))
            } else {
                with_q("").trim().to_string()
            });
            // Operator hygiene on every query: HR noise never reaches the
            // result slots (the ledger would ban it anyway — later is waste).
            let web_queries: Vec<String> = web_queries
                .iter()
                .map(|q| format!("{} {NOISE_EXCLUSIONS}", q.trim()))
                .collect();
            for wq in &web_queries {
                if let Ok(hits) = fm_fetch::websearch::web_search(wq, per_query) {
                    candidates.extend(hits.iter().map(|h| {
                        fm_research::candidate_from_web_hit(
                            h,
                            fm_research::SourceBackend::BasicHttp,
                        )
                    }));
                }
            }
            // The user's own URLs lead the ledger.
            let mut seeded = pinned_candidates(&question);
            seeded.append(&mut candidates);
            let mut candidates = seeded;
            fm_research::upgrade_company_candidates(
                &mut candidates,
                &question_name_tokens(&question),
            );
            // Reserve a quote slot per ticker for comparison, else one snapshot slot.
            let quote_slots = if comparison { tickers.len() } else { 1 };
            let mut ledger = fm_research::assemble_ledger(
                candidates,
                max_sources.saturating_sub(quote_slots as u32).max(1),
                2,
            );
            for t in &tickers {
                // A web hit for the same quote page may already be in the
                // ledger — never append a duplicate synthetic row.
                let quote_url = format!("https://finance.yahoo.com/quote/{t}");
                if ledger.iter().any(|s| s.canonical_url.trim_end_matches('/') == quote_url) {
                    continue;
                }
                if let Ok(q) = fm_fetch::fetch_quote(t) {
                    let id = format!("S{}", ledger.len() + 1);
                    ledger.push(fm_research::synthetic_source(
                        id,
                        format!("https://finance.yahoo.com/quote/{t}"),
                        format!("{t} market quote"),
                        render_quote(&q, &stamp()),
                        fm_research::research::SourceKind::Secondary,
                        stamp(),
                    ));
                    if !comparison {
                        break;
                    }
                }
            }
            ledger
        })
        .await
        .unwrap_or_default()
    }

    /// Deal acquisition: a deal-tuned web search from the parsed target/acquirer
    /// (falling back to the question), read as ordinary web sources.
    async fn deal_search(&self) -> Vec<SourceRecord> {
        let parties = format!("{} {}", self.acquirer.trim(), self.target.trim());
        let query = if parties.trim().is_empty() {
            self.question.clone()
        } else {
            format!("{} acquisition merger deal terms", parties.trim())
        };
        let per_query = self.per_query_results;
        let max_sources = self.max_sources;
        tauri::async_runtime::spawn_blocking(move || {
            let mut candidates = Vec::new();
            if let Ok(hits) = fm_fetch::websearch::web_search(&query, per_query) {
                candidates.extend(hits.iter().map(|h| {
                    fm_research::candidate_from_web_hit(h, fm_research::SourceBackend::BasicHttp)
                }));
            }
            fm_research::assemble_ledger(candidates, max_sources, 2)
        })
        .await
        .unwrap_or_default()
    }
}

impl ResearchBackend for HttpBackend {
    async fn plan(&self, request: &ResearchRequest) -> Option<ResearchPlan> {
        // Deterministic primary-source-first plan (no model round). Doctrine:
        // the company's own words — IR pages, press/earnings releases,
        // presentations — are searched BEFORE anyone else's commentary; the
        // open web comes last. The reducer clamps to the depth budget
        // (Standard 4, Deep 8), so the ordering here is the priority order.
        let q = request.question.trim().to_string();
        if q.is_empty() {
            return None;
        }
        // Exchange-suffixed local tickers (MC.PA, 7203.T) are search NOISE —
        // Bing read "MC.PA LVMH…" as Minecraft. The question names the company;
        // only bare US-style tickers help a query.
        let subject = self
            .tickers
            .iter()
            .filter(|t| !t.contains('.'))
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let with_subject = |tail: &str| -> String {
            if subject.is_empty() {
                format!("{q} {tail}")
            } else {
                format!("{subject} {q} {tail}")
            }
        };
        let mut queries = vec![
            with_subject("investor relations"),
            with_subject("press release"),
            q.clone(),
            with_subject("news analysis"),
        ];
        // "What did management say…" questions live in the call transcript —
        // hunt it at any depth, not just Deep.
        let ql = q.to_lowercase();
        if ["say", "said", "mention", "discuss", "comment", "call", "guidance"]
            .iter()
            .any(|k| ql.contains(k))
        {
            queries.insert(2, with_subject("earnings call transcript"));
        }
        if request.depth == fm_research::research::ResearchDepth::Deep {
            queries.push(with_subject("investor presentation"));
            queries.push(with_subject("earnings call transcript"));
            queries.push(with_subject("SEC filing"));
            queries.push(with_subject("annual report"));
        }
        let queries = queries
            .into_iter()
            .map(|q| format!("{} {NOISE_EXCLUSIONS}", q.trim()))
            .collect();
        Some(ResearchPlan {
            queries,
            required_source_types: vec![],
        })
    }

    async fn search(&self, queries: &[String]) -> Vec<SourceRecord> {
        match self.mode {
            ResearchMode::Filing => {
                let tickers = self.tickers.clone();
                let forms = self.filing_forms.clone();
                let per_ticker = self.max_sources.max(1) as usize;
                let candidates = tauri::async_runtime::spawn_blocking(move || {
                    let form_refs: Vec<&str> = if forms.is_empty() {
                        vec!["10-K", "10-Q", "8-K", "20-F", "40-F"]
                    } else {
                        forms.iter().map(String::as_str).collect()
                    };
                    let mut cands = Vec::new();
                    for t in &tickers {
                        if let Ok(cik) = fm_fetch::edgar::cik_from_ticker(t) {
                            if let Ok(filings) =
                                fm_fetch::edgar::search_filings(&cik, &form_refs, per_ticker)
                            {
                                cands
                                    .extend(filings.iter().map(fm_research::candidate_from_filing));
                            }
                        }
                    }
                    cands
                })
                .await
                .unwrap_or_default();
                // Every filing is on sec.gov — raise the per-domain cap to the budget.
                fm_research::assemble_ledger(candidates, self.max_sources, self.max_sources)
            }
            ResearchMode::Company | ResearchMode::Earnings | ResearchMode::Comparison => {
                self.fused_search().await
            }
            ResearchMode::Deal => self.deal_search().await,
            _ => {
                let queries = queries.to_vec();
                let per_query = self.per_query_results;
                let hits = tauri::async_runtime::spawn_blocking(move || {
                    let mut all = Vec::new();
                    for q in &queries {
                        if let Ok(hits) = fm_fetch::websearch::web_search(q, per_query) {
                            all.extend(hits);
                        }
                    }
                    all
                })
                .await
                .unwrap_or_default();
                let mut candidates: Vec<fm_research::Candidate> = hits
                    .iter()
                    .map(|h| {
                        fm_research::candidate_from_web_hit(
                            h,
                            fm_research::SourceBackend::BasicHttp,
                        )
                    })
                    .collect();
                // The user's own URLs lead the ledger.
                let mut seeded = pinned_candidates(&self.question);
                seeded.append(&mut candidates);
                let mut candidates = seeded;
                fm_research::upgrade_company_candidates(
                    &mut candidates,
                    &question_name_tokens(&self.question),
                );
                fm_research::assemble_ledger(candidates, self.max_sources, 2)
            }
        }
    }

    async fn read(&self, ledger: Vec<SourceRecord>) -> Vec<SourceRecord> {
        // BasicHttp/EDGAR reads run with concurrency 3; order is restored to
        // the input ledger order (Phase 3.4). A blocked/unreadable source gets
        // one retry through the Roam MCP browser when configured — the user's
        // real browser sees the live page bots are walled from.
        const READ_CONCURRENCY: usize = 3;
        let question = self.question.clone();
        let roam = self.roam.clone();
        let mut out: Vec<Option<SourceRecord>> = (0..ledger.len()).map(|_| None).collect();
        let mut next = 0usize;
        while next < ledger.len() {
            let end = (next + READ_CONCURRENCY).min(ledger.len());
            let chunk: Vec<(usize, SourceRecord)> = ledger[next..end]
                .iter()
                .cloned()
                .enumerate()
                .map(|(i, r)| (next + i, r))
                .collect();
            let mut handles = Vec::with_capacity(chunk.len());
            for (idx, mut rec) in chunk {
                let q = question.clone();
                let roam = roam.clone();
                handles.push(tauri::async_runtime::spawn_blocking(move || {
                    // A pre-read synthetic source (market quote) passes through.
                    if rec.status == fm_research::research::SourceStatus::Read
                        && rec.excerpt.is_some()
                    {
                        return (idx, rec);
                    }
                    let url = rec.requested_url.clone();
                    let outcome = match fm_research::validate_request_url(&url) {
                        Err(e) => fm_research::read_outcome_failed(
                            None,
                            stamp(),
                            format!("url_rejected:{e:?}"),
                        ),
                        Ok(v) if host_resolves_to_forbidden(&v.host) => {
                            fm_research::read_outcome_failed(
                                None,
                                stamp(),
                                "ssrf_blocked".to_string(),
                            )
                        }
                        Ok(_) if fm_research::is_edgar_archive_url(&url) => {
                            match fm_fetch::edgar::fetch_filing_doc(&url) {
                                Ok(text) => {
                                    let page = fm_fetch::FetchedPage {
                                        title: rec.title.clone(),
                                        text: fm_research::select_filing_excerpt(&text, &q, 4000),
                                        status: fm_fetch::PageStatus::Ok,
                                    };
                                    fm_research::read_outcome_from_page(
                                        &page,
                                        Some(url.clone()),
                                        stamp(),
                                        4000,
                                    )
                                }
                                Err(_) => fm_research::read_outcome_failed(
                                    None,
                                    stamp(),
                                    "edgar_fetch_error".to_string(),
                                ),
                            }
                        }
                        Ok(_) if is_pdf_url(&url) => match fetch_pdf_page(&url, &q) {
                            Ok(page) => fm_research::read_outcome_from_page(
                                &page,
                                Some(url.clone()),
                                stamp(),
                                4000,
                            ),
                            Err(code) => fm_research::read_outcome_failed(
                                None,
                                stamp(),
                                format!("pdf:{code}"),
                            ),
                        },
                        Ok(_) => match fm_fetch::fetch_page(&url) {
                            Ok(page) => fm_research::read_outcome_from_page(
                                &page,
                                Some(url.clone()),
                                stamp(),
                                4000,
                            ),
                            Err(_) => fm_research::read_outcome_failed(
                                None,
                                stamp(),
                                "fetch_error".to_string(),
                            ),
                        },
                    };
                    let outcome = match (&roam, outcome.status) {
                        (
                            Some(roam),
                            fm_research::research::SourceStatus::Blocked
                            | fm_research::research::SourceStatus::Failed,
                        ) => {
                            // Live-browser retry: the user's Roam browser reads
                            // the page a human actually sees. Only for sources
                            // the plain fetch could not read — never everything.
                            match roam(&url, &q) {
                                Some(page)
                                    if !page.text.trim().is_empty()
                                        && page.status == fm_fetch::PageStatus::Ok =>
                                {
                                    fm_research::read_outcome_from_page(
                                        &page,
                                        Some(url.clone()),
                                        stamp(),
                                        4000,
                                    )
                                }
                                _ => outcome,
                            }
                        }
                        _ => outcome,
                    };
                    rec.status = outcome.status;
                    rec.final_url = outcome.final_url;
                    rec.retrieved_at = outcome.retrieved_at;
                    rec.excerpt = outcome.excerpt;
                    rec.error_code = outcome.error_code;
                    (idx, rec)
                }));
            }
            for h in handles {
                if let Ok((idx, rec)) = h.await {
                    out[idx] = Some(rec);
                }
            }
            next = end;
        }
        out.into_iter().flatten().collect()
    }
}

// ── Research → modeling bridge (Phase 5.6) ──────────────────────────────────

/// Convert a validated [`SuggestedAssumption`] into an analyst-grid
/// [`fm_build::AssumptionOverride`]: one cell per projection year (`Some` for a
/// suggested year, `None` to keep the engine-derived value), tagged with
/// `Research` provenance carrying the (deduped) citation source ids.
pub fn suggested_to_override(
    s: &fm_research::bridge::SuggestedAssumption,
    proj_years: &[i32],
) -> fm_build::AssumptionOverride {
    use std::collections::HashMap;
    let year_val: HashMap<i32, f64> = s
        .years
        .iter()
        .copied()
        .zip(s.values.iter().copied())
        .collect();
    let values = proj_years
        .iter()
        .map(|y| year_val.get(y).copied())
        .collect();
    let mut source_ids: Vec<String> = Vec::new();
    for c in &s.citations {
        if !source_ids.contains(&c.source_id) {
            source_ids.push(c.source_id.clone());
        }
    }
    fm_build::AssumptionOverride {
        key: s.key.field().to_string(),
        values,
        provenance: Some(fm_build::AssumptionProvenance {
            origin: fm_build::AssumptionOrigin::Research,
            source_ids,
        }),
    }
}

/// Validate each accepted suggestion against the projection horizon and the
/// `Read` source ids; convert the valid rows to overrides and report the rejected
/// ones as `(index, reason)`. ONLY valid rows yield an override, so a malformed
/// suggestion can never perturb a model (Phase 5.6 gate).
pub fn accept_valid_suggestions(
    suggestions: &[fm_research::bridge::SuggestedAssumption],
    proj_years: &[i32],
    read_source_ids: &std::collections::HashSet<&str>,
) -> (
    Vec<fm_build::AssumptionOverride>,
    Vec<(usize, fm_research::bridge::SuggestionReject)>,
) {
    let mut overrides = Vec::new();
    let mut rejected = Vec::new();
    for (i, s) in suggestions.iter().enumerate() {
        match fm_research::bridge::validate_suggested_assumption(s, proj_years, read_source_ids) {
            Ok(()) => overrides.push(suggested_to_override(s, proj_years)),
            Err(e) => rejected.push((i, e)),
        }
    }
    (overrides, rejected)
}

/// Human-readable reason string for a rejected row (UI grid label).
fn reject_reason(r: fm_research::bridge::SuggestionReject) -> &'static str {
    use fm_research::bridge::SuggestionReject as R;
    match r {
        R::Empty => "empty years/values",
        R::LengthMismatch => "years/values length mismatch",
        R::DuplicateYear => "duplicate year",
        R::YearOutOfHorizon => "year outside projection horizon",
        R::NonFiniteValue => "non-finite value",
        R::ValueOutOfBounds => "value outside driver bounds",
        R::UnitMismatch => "unit mismatch",
        R::NoCitation => "no citation",
        R::CitationNotRead => "citation not to a Read source",
    }
}

/// Validate research-suggested assumptions for the analyst review grid
/// (Phase 5.6). Returns one verdict per row: `{ index, ok, reason?, override? }`.
/// The UI renders the grid and requires INDIVIDUAL accept; only rows the user
/// accepts (from the `ok` set) are later applied to a model. Malformed rows are
/// rejected here and can never reach a workbook.
#[tauri::command(rename_all = "snake_case")]
pub fn review_suggested_assumptions(
    suggestions: Vec<fm_research::bridge::SuggestedAssumption>,
    proj_years: Vec<i32>,
    read_source_ids: Vec<String>,
) -> crate::error::AppResult<String> {
    use std::collections::HashSet;
    let read: HashSet<&str> = read_source_ids.iter().map(String::as_str).collect();
    let rows: Vec<Value> = suggestions
        .iter()
        .enumerate()
        .map(|(i, s)| {
            match fm_research::bridge::validate_suggested_assumption(s, &proj_years, &read) {
                Ok(()) => {
                    let ov = suggested_to_override(s, &proj_years);
                    json!({
                        "index": i,
                        "ok": true,
                        "key": s.key.field(),
                        "rationale": s.rationale,
                        "override": serde_json::to_value(&ov).unwrap_or(Value::Null),
                    })
                }
                Err(e) => json!({
                    "index": i,
                    "ok": false,
                    "key": s.key.field(),
                    "reason": reject_reason(e),
                }),
            }
        })
        .collect();
    Ok(json!({ "rows": rows }).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use fm_research::bridge::{AssumptionKey, AssumptionUnit, SuggestedAssumption};
    use fm_research::machine::ResearchBudgets;
    use fm_research::research::{
        AnswerSection, CitationRef, CitedParagraph, ResearchConfidence, ResearchDepth,
        ResearchMode, ResearchOutput, SourceBackend, SourceKind, SourceStatus,
    };

    fn suggestion(years: Vec<i32>, values: Vec<f64>, cite_id: &str) -> SuggestedAssumption {
        SuggestedAssumption {
            key: AssumptionKey::RevenueGrowthPct,
            years,
            values,
            unit: AssumptionUnit::Percent,
            rationale: "Guidance implies deceleration.".into(),
            citations: vec![CitationRef {
                source_id: cite_id.into(),
                quote: "revenue growth of 12 percent".into(),
            }],
            confidence: ResearchConfidence::Medium,
        }
    }

    #[test]
    fn review_grid_accepts_valid_and_rejects_malformed_rows() {
        let good = suggestion(vec![2026, 2027], vec![12.0, 10.0], "S1");
        // Length mismatch → rejected, never an override.
        let bad = suggestion(vec![2026, 2027], vec![12.0], "S1");
        // Citation not to a Read source → rejected.
        let unread = suggestion(vec![2026], vec![9.0], "S9");
        let out = review_suggested_assumptions(
            vec![good, bad, unread],
            vec![2026, 2027, 2028],
            vec!["S1".to_string(), "S2".to_string()],
        )
        .unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0]["ok"], serde_json::json!(true));
        assert!(rows[0]["override"]["provenance"]["origin"] == serde_json::json!("research"));
        assert!(rows[0]["override"]["provenance"]["source_ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s == "S1"));
        assert_eq!(rows[1]["ok"], serde_json::json!(false));
        assert!(rows[1]["reason"].as_str().unwrap().contains("length"));
        assert_eq!(rows[2]["ok"], serde_json::json!(false));
        assert!(rows[2]["reason"].as_str().unwrap().contains("Read"));
    }

    fn request(depth: ResearchDepth) -> ResearchRequest {
        ResearchRequest {
            question: "Research the current investment case for Nvidia.".into(),
            mode: ResearchMode::Web,
            tickers: vec!["NVDA".into()],
            periods: vec![],
            filing_forms: vec![],
            target: None,
            acquirer: None,
            depth,
        }
    }

    fn source(id: &str, kind: SourceKind, status: SourceStatus, excerpt: &str) -> SourceRecord {
        SourceRecord {
            id: id.into(),
            requested_url: format!("https://ex.com/{id}"),
            final_url: Some(format!("https://ex.com/{id}")),
            canonical_url: format!("https://ex.com/{id}"),
            title: id.into(),
            domain: "ex.com".into(),
            retrieved_at: "t".into(),
            status,
            kind,
            backend: SourceBackend::BasicHttp,
            snippet: None,
            excerpt: Some(excerpt.into()),
            error_code: None,
        }
    }

    struct FakeBackend {
        plan: Option<ResearchPlan>,
        results: Vec<SourceRecord>,
    }
    impl ResearchBackend for FakeBackend {
        async fn plan(&self, _request: &ResearchRequest) -> Option<ResearchPlan> {
            self.plan.clone()
        }
        async fn search(&self, _queries: &[String]) -> Vec<SourceRecord> {
            self.results.clone()
        }
        async fn read(&self, ledger: Vec<SourceRecord>) -> Vec<SourceRecord> {
            ledger // already Read in this fixture
        }
    }

    struct FakeSynth {
        outcome: Result<ResearchAnswer, SynthReject>,
    }
    impl ResearchSynthesizer for FakeSynth {
        async fn synthesize(
            &self,
            _attempt: u32,
            _read: &[SourceRecord],
        ) -> Result<ResearchAnswer, SynthReject> {
            self.outcome.clone()
        }
    }

    fn answer() -> ResearchAnswer {
        ResearchAnswer {
            question: "Research the current investment case for Nvidia.".into(),
            summary: CitedParagraph {
                text: "Growth is strong.".into(),
                citations: vec![CitationRef {
                    source_id: "S1".into(),
                    quote: "revenue grew".into(),
                }],
            },
            sections: vec![AnswerSection {
                heading: "Risks".into(),
                paragraphs: vec![CitedParagraph {
                    text: "Competition is a risk.".into(),
                    citations: vec![CitationRef {
                        source_id: "S2".into(),
                        quote: "competition".into(),
                    }],
                }],
            }],
            sources: vec![
                source(
                    "S1",
                    SourceKind::Regulatory,
                    SourceStatus::Read,
                    "revenue grew",
                ),
                source(
                    "S2",
                    SourceKind::Newswire,
                    SourceStatus::Read,
                    "competition",
                ),
            ],
            limitations: vec![],
            confidence: ResearchConfidence::High,
            generated_at: "t".into(),
            model: "test/model".into(),
        }
    }

    #[tokio::test]
    async fn async_driver_pumps_machine_to_validated_answer() {
        let machine = ResearchMachine::new(
            request(ResearchDepth::Standard),
            ResearchBudgets::from_depth(ResearchDepth::Standard),
            "t",
        );
        let backend = FakeBackend {
            plan: Some(ResearchPlan {
                queries: vec!["nvidia".into()],
                required_source_types: vec![],
            }),
            results: vec![
                source(
                    "S1",
                    SourceKind::Regulatory,
                    SourceStatus::Read,
                    "revenue grew",
                ),
                source(
                    "S2",
                    SourceKind::Newswire,
                    SourceStatus::Read,
                    "competition",
                ),
            ],
        };
        let synth = FakeSynth {
            outcome: Ok(answer()),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let terminal = run_research(
            machine,
            request(ResearchDepth::Standard),
            &backend,
            &synth,
            &cancel,
            &|_| {},
        )
        .await;
        match terminal {
            Action::Done(ResearchOutput::Answer(a)) => assert_eq!(a.sources.len(), 2),
            other => panic!("expected Done(Answer), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn seeded_backend_skips_search_read_and_resynthesizes() {
        // A SeededBackend returns the stored ledger from `search` and passes it
        // through `read` — proving a Synthesizing retry re-runs only synthesis
        // on the stored sources (no network search/read).
        let ledger = vec![
            source(
                "S1",
                SourceKind::Regulatory,
                SourceStatus::Read,
                "revenue grew",
            ),
            source(
                "S2",
                SourceKind::Newswire,
                SourceStatus::Read,
                "competition",
            ),
        ];
        let backend = SeededBackend {
            ledger: ledger.clone(),
        };
        // plan() returns None so the machine goes straight to search→read→synth.
        assert!(backend
            .plan(&request(ResearchDepth::Standard))
            .await
            .is_none());
        assert_eq!(backend.search(&[]).await.len(), 2);
        assert_eq!(backend.read(ledger.clone()).await, ledger);
        let machine = ResearchMachine::new(
            request(ResearchDepth::Standard),
            ResearchBudgets::from_depth(ResearchDepth::Standard),
            "t",
        );
        let synth = FakeSynth {
            outcome: Ok(answer()),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let terminal = run_research(
            machine,
            request(ResearchDepth::Standard),
            &backend,
            &synth,
            &cancel,
            &|_| {},
        )
        .await;
        match terminal {
            Action::Done(ResearchOutput::Answer(a)) => assert_eq!(a.sources.len(), 2),
            other => panic!("expected Done(Answer), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn async_driver_all_blocked_yields_digest() {
        let machine = ResearchMachine::new(
            request(ResearchDepth::Quick),
            ResearchBudgets::from_depth(ResearchDepth::Quick),
            "t",
        );
        let backend = FakeBackend {
            plan: None,
            results: vec![source(
                "S1",
                SourceKind::Newswire,
                SourceStatus::Blocked,
                "",
            )],
        };
        let synth = FakeSynth {
            outcome: Err(SynthReject::Empty),
        };
        let terminal = run_research(
            machine,
            request(ResearchDepth::Quick),
            &backend,
            &synth,
            &tokio_util::sync::CancellationToken::new(),
            &|_| {},
        )
        .await;
        assert!(matches!(terminal, Action::Done(ResearchOutput::Digest(_))));
    }

    #[tokio::test]
    async fn async_driver_twice_invalid_synthesis_yields_digest() {
        let machine = ResearchMachine::new(
            request(ResearchDepth::Quick),
            ResearchBudgets::from_depth(ResearchDepth::Quick),
            "t",
        );
        let backend = FakeBackend {
            plan: None,
            results: vec![source(
                "S1",
                SourceKind::Regulatory,
                SourceStatus::Read,
                "x",
            )],
        };
        // Synthesis always rejects → one repair → digest.
        let synth = FakeSynth {
            outcome: Err(SynthReject::UncitedParagraph),
        };
        let terminal = run_research(
            machine,
            request(ResearchDepth::Quick),
            &backend,
            &synth,
            &tokio_util::sync::CancellationToken::new(),
            &|_| {},
        )
        .await;
        match terminal {
            Action::Done(ResearchOutput::Digest(d)) => {
                assert!(d.limitations[0].starts_with("Grounding:"), "{:?}", d.limitations);
                assert_eq!(
                    d.limitations[1],
                    "The selected model could not produce a validated synthesis".to_string()
                );
            }
            other => panic!("expected Done(Digest), got {other:?}"),
        }
    }

    #[tokio::test]
    async fn block_on_from_spawn_blocking_bridge_works() {
        // Mirrors chat_send_blocking's runtime bridge: a blocking-pool thread runs
        // the async driver via `tauri::async_runtime::block_on`. Proves the bridge
        // before wiring the chat dispatch (no nested-runtime panic).
        let terminal = tauri::async_runtime::spawn_blocking(|| {
            let machine = ResearchMachine::new(
                request(ResearchDepth::Quick),
                ResearchBudgets::from_depth(ResearchDepth::Quick),
                "t",
            );
            let backend = FakeBackend {
                plan: None,
                results: vec![source(
                    "S1",
                    SourceKind::Regulatory,
                    SourceStatus::Read,
                    "revenue grew",
                )],
            };
            let synth = FakeSynth {
                outcome: Ok(answer()),
            };
            let cancel = tokio_util::sync::CancellationToken::new();
            tauri::async_runtime::block_on(run_research(
                machine,
                request(ResearchDepth::Quick),
                &backend,
                &synth,
                &cancel,
                &|_| {},
            ))
        })
        .await
        .expect("join");
        assert!(matches!(terminal, Action::Done(_)));
    }

    /// A backend whose search stage sleeps longer than any Quick deadline.
    /// Proves the driver aborts the await via `tokio::time::timeout` and
    /// terminates with a Deadline digest instead of hanging.
    struct SlowSearchBackend;
    impl ResearchBackend for SlowSearchBackend {
        async fn plan(&self, _request: &ResearchRequest) -> Option<ResearchPlan> {
            None
        }
        async fn search(&self, _queries: &[String]) -> Vec<SourceRecord> {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            vec![]
        }
        async fn read(&self, ledger: Vec<SourceRecord>) -> Vec<SourceRecord> {
            ledger
        }
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn async_driver_stage_timeout_yields_deadline_digest() {
        // Quick deadline is 30s. With a paused clock we advance past it while
        // the slow search is in-flight; the timeout fires and the reducer
        // emits an honest deadline digest.
        let machine = ResearchMachine::new(
            request(ResearchDepth::Quick),
            ResearchBudgets::from_depth(ResearchDepth::Quick),
            "t",
        );
        let synth = FakeSynth {
            outcome: Err(SynthReject::Empty),
        };
        let cancel = tokio_util::sync::CancellationToken::new();
        let fut = run_research(
            machine,
            request(ResearchDepth::Quick),
            &SlowSearchBackend,
            &synth,
            &cancel,
            &|_| {},
        );
        tokio::pin!(fut);
        // Poll once so the search stage arms its timeout (creates Instant +
        // registers the tokio timeout), then jump past the 30s Quick deadline.
        // Without this poll, Instant/timeout would start after advance and hang.
        tokio::select! {
            biased;
            terminal = &mut fut => panic!("driver finished before advance: {terminal:?}"),
            _ = tokio::task::yield_now() => {}
        }
        tokio::time::advance(std::time::Duration::from_secs(31)).await;
        let terminal = fut.await;
        match terminal {
            Action::Done(ResearchOutput::Digest(d)) => {
                assert!(
                    d.limitations.iter().any(|l| l.contains("ran out of research time")),
                    "expected deadline limitation, got {:?}",
                    d.limitations
                );
            }
            other => panic!("expected Done(Digest) on deadline, got {other:?}"),
        }
    }

    #[test]
    fn suggested_to_override_maps_years_and_tags_research_provenance() {
        use fm_research::bridge::{AssumptionKey, AssumptionUnit, SuggestedAssumption};
        let s = SuggestedAssumption {
            key: AssumptionKey::RevenueGrowthPct,
            years: vec![2026, 2028],
            values: vec![12.0, 8.0],
            unit: AssumptionUnit::Percent,
            rationale: "r".into(),
            citations: vec![
                CitationRef {
                    source_id: "S1".into(),
                    quote: "q".into(),
                },
                CitationRef {
                    source_id: "S1".into(),
                    quote: "q2".into(),
                },
            ],
            confidence: ResearchConfidence::Medium,
        };
        let ov = suggested_to_override(&s, &[2026, 2027, 2028]);
        assert_eq!(ov.key, "revenue_growth_pct");
        // 2027 is untouched (None keeps the engine value); 2026/2028 set.
        assert_eq!(ov.values, vec![Some(12.0), None, Some(8.0)]);
        let prov = ov.provenance.expect("provenance");
        assert_eq!(prov.origin, fm_build::AssumptionOrigin::Research);
        assert_eq!(prov.source_ids, vec!["S1".to_string()]); // deduped
    }

    #[test]
    fn accept_valid_suggestions_admits_only_valid_rows() {
        use fm_research::bridge::{
            AssumptionKey, AssumptionUnit, SuggestedAssumption, SuggestionReject,
        };
        let ok = SuggestedAssumption {
            key: AssumptionKey::TaxRatePct,
            years: vec![2026],
            values: vec![21.0],
            unit: AssumptionUnit::Percent,
            rationale: "r".into(),
            citations: vec![CitationRef {
                source_id: "S1".into(),
                quote: "q".into(),
            }],
            confidence: ResearchConfidence::High,
        };
        // Invalid: citation to a non-Read source.
        let bad = SuggestedAssumption {
            citations: vec![CitationRef {
                source_id: "S9".into(),
                quote: "q".into(),
            }],
            ..ok.clone()
        };
        let read: std::collections::HashSet<&str> = std::collections::HashSet::from(["S1"]);
        let (overrides, rejected) = accept_valid_suggestions(&[ok, bad], &[2026, 2027], &read);
        assert_eq!(overrides.len(), 1);
        assert_eq!(overrides[0].key, "tax_rate_pct");
        assert_eq!(rejected, vec![(1, SuggestionReject::CitationNotRead)]);
    }
}

#[cfg(test)]
mod plan_tests {
    use super::*;
    use fm_research::research::{ResearchDepth, ResearchMode};

    fn backend(tickers: &[&str]) -> HttpBackend {
        HttpBackend {
            max_sources: 10,
            per_query_results: 6,
            mode: ResearchMode::Web,
            tickers: tickers.iter().map(|s| s.to_string()).collect(),
            filing_forms: vec![],
            question: String::new(),
            target: String::new(),
            acquirer: String::new(),
            roam: None,
        }
    }

    fn req(depth: ResearchDepth) -> ResearchRequest {
        ResearchRequest {
            question: "tariff impact and China competition".into(),
            mode: ResearchMode::Web,
            tickers: vec!["TSLA".into()],
            periods: vec![],
            filing_forms: vec![],
            target: None,
            acquirer: None,
            depth,
        }
    }

    #[test]
    fn primary_first_plan_targets_company_sources() {
        let plan = tauri::async_runtime::block_on(
            backend(&["TSLA"]).plan(&req(ResearchDepth::Standard)),
        )
        .expect("deterministic plan");
        // The FIRST queries hunt the company's own words; the open web is last.
        assert!(plan.queries[0].contains("investor relations"));
        assert!(plan.queries[1].contains("press release"));
        assert!(plan.queries.iter().all(|q| q.contains("tariff")||q.contains("TSLA")));

        // Operator hygiene: every planned query drops the HR-noise domains,
        // and date operators (unsupported on the DDG fallback) never appear.
        assert!(plan.queries.iter().all(|q| q.contains("-site:linkedin.com")), "{:?}", plan.queries);
        assert!(plan.queries.iter().all(|q| !q.contains("before:") && !q.contains("after:")));
        // Deep widens into presentations, transcripts, and filings.
        let deep = tauri::async_runtime::block_on(
            backend(&["TSLA"]).plan(&req(ResearchDepth::Deep)),
        )
        .unwrap();
        assert!(deep.queries.len() > plan.queries.len());
        assert!(deep.queries.iter().any(|q| q.contains("investor presentation")));
        assert!(deep.queries.iter().any(|q| q.contains("earnings call transcript")));
    }

    /// LIVE (network): the full acquisition path on a real question — plan
    /// queries fire, IR/press/SEC sources enter the ledger ahead of the open
    /// web, and banned domains never appear. Run explicitly:
    /// cargo test --lib live_primary_first_research -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_primary_first_research() {
        let b = backend(&["TSLA"]);
        let r = req(ResearchDepth::Standard);
        let plan = tauri::async_runtime::block_on(b.plan(&r)).expect("plan");
        println!("plan queries: {:#?}", plan.queries);
        assert!(plan.queries.len() >= 4);
        let ledger = tauri::async_runtime::block_on(b.search(&plan.queries));
        println!("ledger ({} sources):", ledger.len());
        for s in &ledger {
            println!("  {} [{:?}] {}", s.id, s.kind, s.canonical_url);
        }
        assert!(!ledger.is_empty(), "search produced no candidates");
        assert!(
            ledger.iter().all(|s| !s.canonical_url.contains("wikipedia")),
            "wikipedia must never enter the ledger"
        );
        let read = tauri::async_runtime::block_on(b.read(ledger));
        let ok = read
            .iter()
            .filter(|s| s.status == fm_research::research::SourceStatus::Read)
            .count();
        println!("read ok: {ok}/{}", read.len());
        for s in &read {
            println!("  {} [{:?}/{:?}] {}", s.id, s.kind, s.status, s.canonical_url);
        }
        assert!(ok >= 1, "at least one source must be readable");
    }

    #[test]
    fn user_urls_become_pinned_candidates() {
        let c = pinned_candidates(
            "give me an overview of https://www.acme-widgets.example/about, and compare with https://en.wikipedia.org/wiki/Acme.",
        );
        // Both extracted (wikipedia is dropped later by the ledger ban, not here);
        // trailing punctuation stripped; pinned set; kind honestly classified.
        assert_eq!(c.len(), 2);
        assert!(c[0].pinned);
        assert_eq!(c[0].url, "https://www.acme-widgets.example/about");
        assert_eq!(c[1].url, "https://en.wikipedia.org/wiki/Acme");
        assert!(pinned_candidates("no links here").is_empty());
    }

    #[test]
    fn pinned_wikipedia_still_never_enters_the_ledger() {
        let cands = pinned_candidates("use https://en.wikipedia.org/wiki/Acme as truth");
        let led = fm_research::assemble_ledger(cands, 10, 10);
        assert!(led.is_empty(), "the wikipedia ban outranks even a pin");
    }

    #[test]
    fn transcript_query_for_spoken_questions() {
        // The req() question asks about "tariff impact and China competition" —
        // no spoken-word cue, so no transcript query at Standard.
        let plain = tauri::async_runtime::block_on(
            backend(&["TSLA"]).plan(&req(ResearchDepth::Standard)),
        )
        .unwrap();
        assert!(!plain.queries.iter().any(|q| q.contains("transcript")));
        // A "what did they say" question hunts the call transcript up front.
        let mut r = req(ResearchDepth::Standard);
        r.question = "did management say anything about tariffs on the call".into();
        let spoken =
            tauri::async_runtime::block_on(backend(&["TSLA"]).plan(&r)).unwrap();
        assert!(
            spoken.queries.iter().any(|q| q.contains("earnings call transcript")),
            "queries: {:?}",
            spoken.queries
        );
    }

    #[test]
    fn empty_question_yields_no_plan() {
        let mut r = req(ResearchDepth::Standard);
        r.question = String::new();
        assert!(
            tauri::async_runtime::block_on(backend(&["TSLA"]).plan(&r)).is_none()
        );
    }
}

#[cfg(test)]
mod pdf_tests {
    use super::*;

    #[test]
    fn pdf_urls_detected_by_path_not_query() {
        assert!(is_pdf_url("https://ir.tesla.com/deck/Q1-2026.pdf"));
        assert!(is_pdf_url("https://x.com/a/B.PDF?dl=1#page=3"));
        assert!(!is_pdf_url("https://x.com/pdf-viewer?file=a.pdf")); // path is not a pdf
        assert!(!is_pdf_url("https://x.com/report.html"));
        assert!(!is_pdf_url("not a url"));
    }

    /// A minimal one-page PDF with a real text layer — built with a correct
    /// xref table (offsets computed, not hand-typed) — pushed through the same
    /// extractor the read path uses.
    #[test]
    fn pdf_text_layer_extracts() {
        let stream = "BT /F1 12 Tf 72 720 Td (Tariff impact rose in Q1 2026) Tj ET";
        let objects = [
            "<</Type/Catalog/Pages 2 0 R>>".to_string(),
            "<</Type/Pages/Kids[3 0 R]/Count 1>>".to_string(),
            "<</Type/Page/Parent 2 0 R/MediaBox[0 0 612 792]/Contents 4 0 R/Resources<</Font<</F1 5 0 R>>>>>>"
                .to_string(),
            format!("<</Length {}>>stream\n{stream}\nendstream", stream.len()),
            "<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>".to_string(),
        ];
        let mut pdf = String::from("%PDF-1.4\n");
        let mut offsets = Vec::new();
        for (i, body) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }
        let xref_at = pdf.len();
        pdf.push_str(&format!("xref\n0 {}\n", objects.len() + 1));
        pdf.push_str("0000000000 65535 f \n");
        for off in &offsets {
            pdf.push_str(&format!("{off:010} 00000 n \n"));
        }
        pdf.push_str(&format!(
            "trailer<</Size {}/Root 1 0 R>>\nstartxref\n{xref_at}\n%%EOF",
            objects.len() + 1
        ));
        let tmp = std::env::temp_dir().join("fm-test-mini.pdf");
        std::fs::write(&tmp, pdf.as_bytes()).unwrap();
        let text = fm_extract::extract_pdf_text(&tmp.to_string_lossy());
        let _ = std::fs::remove_file(&tmp);
        let text = text.expect("mini pdf extracts");
        assert!(text.contains("Tariff impact rose"), "got: {text:?}");
    }

    /// LIVE (network): Earnings-mode acquisition surfaces a call transcript
    /// as an issuer-primary source. Run:
    /// cargo test --lib live_transcript_acquisition -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_transcript_acquisition() {
        use crate::commands::research::ResearchBackend as _;
        use fm_research::research::SourceKind;
        let b = HttpBackend {
            max_sources: 10,
            per_query_results: 6,
            mode: fm_research::research::ResearchMode::Earnings,
            tickers: vec!["TSLA".into()],
            filing_forms: vec![],
            question: "what did management say about tariffs".into(),
            target: String::new(),
            acquirer: String::new(),
            roam: None,
        };
        let ledger = tauri::async_runtime::block_on(b.search(&[]));
        for s in &ledger {
            println!("  {} [{:?}] {}", s.id, s.kind, s.canonical_url);
        }
        assert!(
            ledger.iter().any(|s| s.kind == SourceKind::Primary
                && s.canonical_url.to_lowercase().contains("transcript")),
            "no transcript source in the earnings ledger"
        );
    }

    /// LIVE (network): a non-US issuer (LVMH, Euronext Paris — no EDGAR
    /// mapping) still yields company-primary sources: IR pages, annual
    /// report, presentations. Run:
    /// cargo test --lib live_international_acquisition -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_international_acquisition() {
        use crate::commands::research::ResearchBackend as _;
        use fm_research::research::SourceKind;
        let b = HttpBackend {
            max_sources: 10,
            per_query_results: 6,
            mode: fm_research::research::ResearchMode::Company,
            tickers: vec!["MC.PA".into()],
            filing_forms: vec![],
            question: "what did MC.PA guide for fiscal 2026 revenue".into(),
            target: String::new(),
            acquirer: String::new(),
            roam: None,
        };
        let ledger = tauri::async_runtime::block_on(b.search(&[]));
        for s in &ledger {
            println!("  {} [{:?}] {}", s.id, s.kind, s.canonical_url);
        }
        assert!(!ledger.is_empty(), "no candidates for a non-US issuer");
        assert!(
            ledger.iter().any(|s| matches!(
                s.kind,
                SourceKind::Regulatory | SourceKind::Company | SourceKind::Primary
            ) || s.canonical_url.contains("lvmh")),
            "no company-primary source for LVMH"
        );
    }

    /// LIVE (network): the EXACT failing user case (Veoneer/Magna 2021
    /// synergies) — the ledger must never again contain careers boards,
    /// LinkedIn, or employer-review sites. Run:
    /// cargo test --lib live_no_careers_pages -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_no_careers_pages() {
        use crate::commands::research::ResearchBackend as _;
        let b = HttpBackend {
            max_sources: 10,
            per_query_results: 6,
            mode: fm_research::research::ResearchMode::Web,
            tickers: vec![],
            filing_forms: vec![],
            question: "Veoneer Magna M&A transaction July 2021 cost synergies announced".into(),
            target: String::new(),
            acquirer: String::new(),
            roam: None,
        };
        let r = ResearchRequest {
            question: b.question.clone(),
            mode: fm_research::research::ResearchMode::Web,
            tickers: vec![],
            periods: vec![],
            filing_forms: vec![],
            target: None,
            acquirer: None,
            depth: fm_research::research::ResearchDepth::Standard,
        };
        let plan = tauri::async_runtime::block_on(b.plan(&r)).expect("plan");
        let ledger = tauri::async_runtime::block_on(b.search(&plan.queries));
        for s in &ledger {
            println!("  {} [{:?}] {}", s.id, s.kind, s.canonical_url);
        }
        assert!(!ledger.is_empty(), "no candidates");
        const BANNED: &[&str] = &[
            "teamtailor", "linkedin", "ambitionbox", "glassdoor", "indeed",
            "greenhouse", "lever.co", "/jobs", "/careers",
        ];
        for s in &ledger {
            let u = s.canonical_url.to_lowercase();
            assert!(
                !BANNED.iter().any(|b| u.contains(b)),
                "banned source leaked into the ledger: {u}"
            );
        }
    }

    /// LIVE (network): a real public PDF through the full fetch→extract→excerpt
    /// path. Run: cargo test --lib live_pdf_read -- --ignored --nocapture
    #[test]
    #[ignore]
    fn live_pdf_read() {
        let page = fetch_pdf_page(
            "https://www.apple.com/newsroom/pdfs/fy2024-q4/FY24_Q4_Consolidated_Financial_Statements.pdf",
            "total net sales",
        )
        .expect("live pdf fetch+extract");
        println!("title: {} · excerpt {} chars", page.title, page.text.len());
        assert!(page.text.len() > 500);
        assert!(page.title.ends_with(".pdf"));
    }

    #[test]
    fn quote_if_phrase_wraps_multiword_names() {
        assert_eq!(quote_if_phrase("Toyota Motor Corporation"), "\"Toyota Motor Corporation\"");
        assert_eq!(quote_if_phrase("SAP"), "SAP");
        assert_eq!(quote_if_phrase("\"Already Quoted\""), "\"Already Quoted\"");
    }
}
