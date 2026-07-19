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
}

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
            for t in &tickers {
                if let Ok(cik) = fm_fetch::edgar::cik_from_ticker(t) {
                    if let Ok(filings) = fm_fetch::edgar::search_filings(&cik, forms, filing_limit)
                    {
                        candidates.extend(filings.iter().map(fm_research::candidate_from_filing));
                    }
                }
            }
            // Company-authored sources FIRST (IR + earnings/press releases),
            // then the independent web — the analyst's order of evidence.
            let subject = tickers.join(" ");
            let mut web_queries: Vec<String> = vec![
                format!("{subject} investor relations {question}"),
                if earnings {
                    format!("{subject} earnings release shareholder letter")
                } else {
                    format!("{subject} press release {question}")
                },
            ];
            web_queries.push(if earnings {
                format!("{subject} latest quarterly earnings results guidance")
            } else if comparison {
                format!("{} comparison", tickers.join(" vs "))
            } else {
                format!("{subject} {question}")
            });
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
            // Reserve a quote slot per ticker for comparison, else one snapshot slot.
            let quote_slots = if comparison { tickers.len() } else { 1 };
            let mut ledger = fm_research::assemble_ledger(
                candidates,
                max_sources.saturating_sub(quote_slots as u32).max(1),
                2,
            );
            for t in &tickers {
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
        let subject = self.tickers.join(" ");
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
        if request.depth == fm_research::research::ResearchDepth::Deep {
            queries.push(with_subject("investor presentation"));
            queries.push(with_subject("earnings call transcript"));
            queries.push(with_subject("SEC filing"));
            queries.push(with_subject("annual report"));
        }
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
                let candidates: Vec<fm_research::Candidate> = hits
                    .iter()
                    .map(|h| {
                        fm_research::candidate_from_web_hit(
                            h,
                            fm_research::SourceBackend::BasicHttp,
                        )
                    })
                    .collect();
                fm_research::assemble_ledger(candidates, self.max_sources, 2)
            }
        }
    }

    async fn read(&self, ledger: Vec<SourceRecord>) -> Vec<SourceRecord> {
        // BasicHttp/EDGAR reads run with concurrency 3; order is restored to the
        // input ledger order (Phase 3.4). MCP is not used on this path.
        const READ_CONCURRENCY: usize = 3;
        let question = self.question.clone();
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
                assert_eq!(
                    d.limitations,
                    vec!["The selected model could not produce a validated synthesis".to_string()]
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
        // Deep widens into presentations, transcripts, and filings.
        let deep = tauri::async_runtime::block_on(
            backend(&["TSLA"]).plan(&req(ResearchDepth::Deep)),
        )
        .unwrap();
        assert!(deep.queries.len() > plan.queries.len());
        assert!(deep.queries.iter().any(|q| q.contains("investor presentation")));
        assert!(deep.queries.iter().any(|q| q.contains("earnings call transcript")));
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
