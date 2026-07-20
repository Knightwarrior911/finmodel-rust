//! Deterministic model routing across run roles (Task 1.5).
//!
//! Settings evolve from one implicit model into explicit role profiles:
//! `orchestrator` (plans/synthesizes), `worker` (isolated delegated tasks),
//! `verifier` (optional; never replaces deterministic checks), and an ordered
//! `fallbacks` list. Every profile records its provider base, model, context
//! window, tool/structured-output capability, optional cost metadata, and a
//! **credential reference** — the OS-credential-store account name, never a raw
//! key. Routing is deterministic; one model may fill multiple roles.

use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::commands::settings::Settings;

/// A run role a model profile can serve.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRole {
    Orchestrator,
    Worker,
    Verifier,
}

/// One configured model profile. `credential_ref` names an OS-credential-store
/// account (see [`crate::commands::secrets`]); the secret itself never lives here.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelProfile {
    /// OpenAI-compatible provider root, e.g. `https://openrouter.ai/api/v1`.
    pub provider_base: String,
    pub model: String,
    /// Model context window in tokens (0 = unknown; router treats as conservative).
    #[serde(default)]
    pub context_window: u32,
    #[serde(default)]
    pub native_tools: bool,
    #[serde(default)]
    pub structured_output: bool,
    #[serde(default)]
    pub cost_per_mtok_in: Option<f64>,
    #[serde(default)]
    pub cost_per_mtok_out: Option<f64>,
    /// OS credential-store account name for this profile's key — NEVER the key.
    pub credential_ref: String,
}

/// The resolved role roster. The orchestrator is mandatory; worker/verifier fall
/// back to it when unset (one model may serve multiple roles). Ordered
/// `fallbacks` are rotated to only on classified failover (Task 6.1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelRoster {
    pub orchestrator: ModelProfile,
    #[serde(default)]
    pub worker: Option<ModelProfile>,
    #[serde(default)]
    pub verifier: Option<ModelProfile>,
    #[serde(default)]
    pub fallbacks: Vec<ModelProfile>,
}

impl ModelRoster {
    /// Deterministically resolve the profile for a role. Worker/verifier default
    /// to the orchestrator so an unconfigured role still runs.
    pub fn route(&self, role: ModelRole) -> &ModelProfile {
        match role {
            ModelRole::Orchestrator => &self.orchestrator,
            ModelRole::Worker => self.worker.as_ref().unwrap_or(&self.orchestrator),
            ModelRole::Verifier => self.verifier.as_ref().unwrap_or(&self.orchestrator),
        }
    }

    /// The ordered failover chain for a role: the role's own profile first, then
    /// each configured fallback that is not the same `(provider_base, model)`.
    pub fn fallback_chain(&self, role: ModelRole) -> Vec<&ModelProfile> {
        let primary = self.route(role);
        let mut chain = vec![primary];
        for fb in &self.fallbacks {
            if !(fb.provider_base == primary.provider_base && fb.model == primary.model) {
                chain.push(fb);
            }
        }
        chain
    }

    /// The capability floor a role requires. Orchestrator and worker must support
    /// native tools; the verifier must support structured output. A profile with
    /// unknown (`0`) context window is allowed — the driver clamps conservatively.
    pub fn validate_role(&self, role: ModelRole) -> Result<(), String> {
        let p = self.route(role);
        match role {
            ModelRole::Orchestrator | ModelRole::Worker => {
                if !p.native_tools {
                    return Err(format!(
                        "{role:?} model '{}' lacks native tool-calling",
                        p.model
                    ));
                }
            }
            ModelRole::Verifier => {
                if !p.structured_output {
                    return Err(format!(
                        "verifier model '{}' lacks structured output",
                        p.model
                    ));
                }
            }
        }
        Ok(())
    }

    /// Build a roster from the flat single-model [`Settings`], migrating the one
    /// configured model into the orchestrator profile (backward compatible).
    /// Worker/verifier are left unset (default to orchestrator) unless explicit
    /// profiles are configured in `Settings::model_profiles`.
    pub fn from_settings(s: &Settings) -> Self {
        let cap = s.model_capability.as_ref();
        let native_tools = cap
            .map(|c| c.model_id == s.model && c.native_tools)
            .unwrap_or(false);
        let structured_output = cap
            .map(|c| c.model_id == s.model && c.strict_json)
            .unwrap_or(false);
        let orchestrator = ModelProfile {
            provider_base: crate::commands::settings::provider_base(s),
            model: s.model.clone(),
            context_window: 0,
            native_tools,
            structured_output,
            cost_per_mtok_in: None,
            cost_per_mtok_out: None,
            // The historical single-key account (see secrets::ACCOUNT).
            credential_ref: "openrouter_api_key".to_string(),
        };
        let extra = s.model_profiles.clone().unwrap_or_default();
        ModelRoster {
            orchestrator,
            worker: extra.worker,
            verifier: extra.verifier,
            fallbacks: extra.fallbacks,
        }
    }
}

/// Optional explicit role profiles persisted in settings alongside the flat
/// orchestrator model (Task 1.5). Absent → every role uses the orchestrator.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ModelProfiles {
    #[serde(default)]
    pub worker: Option<ModelProfile>,
    #[serde(default)]
    pub verifier: Option<ModelProfile>,
    #[serde(default)]
    pub fallbacks: Vec<ModelProfile>,
}

/// What to do after a classified provider failure (Task 6.1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RetryDecision {
    /// Retry the same profile after `backoff_ms` (honors server `Retry-After`).
    Retry { backoff_ms: u64 },
    /// Rotate to the next compatible fallback profile.
    Failover,
    /// Stop the run; carries a short, secret-free reason code.
    Stop { reason: &'static str },
}

/// Decide the next action after a provider error. `attempt` is the number of
/// attempts already made on the current profile (1-based). Only retryable
/// categories retry (bounded by `max_retries`, exponential backoff, honoring
/// `retry_after_ms`); only failover categories rotate, and only when a fallback
/// exists. Context overflow is handled by compaction+one-retry upstream, so it
/// stops here. Everything else stops visibly.
pub fn decide_retry(
    err: crate::agent::provider::ProviderError,
    attempt: u32,
    max_retries: u32,
    retry_after_ms: Option<u64>,
    has_fallback: bool,
) -> RetryDecision {
    use crate::agent::provider::ProviderError as E;
    if err == E::ContextOverflow {
        return RetryDecision::Stop {
            reason: "context_overflow",
        };
    }
    if err.is_retryable() && attempt < max_retries {
        let backoff =
            retry_after_ms.unwrap_or_else(|| 500u64.saturating_mul(1u64 << attempt.min(5)));
        return RetryDecision::Retry {
            backoff_ms: backoff,
        };
    }
    if err.is_failover() && has_fallback {
        return RetryDecision::Failover;
    }
    RetryDecision::Stop {
        reason: match err {
            E::Auth => "auth",
            E::Billing => "billing",
            E::RateLimit => "rate_limit",
            E::Capacity => "capacity",
            E::Transport => "transport",
            E::Timeout => "timeout",
            E::ContentFilter => "content_filter",
            E::ToolIncompatible => "tool_incompatible",
            E::ContextOverflow => "context_overflow",
            E::Unknown => "unknown",
        },
    }
}

/// Whether accumulated spend has reached the run's spend cap (Task 6.1). `None`
/// cap means uncapped.
pub fn over_spend_cap(spent_usd: f64, cap_usd: Option<f64>) -> bool {
    matches!(cap_usd, Some(cap) if spent_usd >= cap)
}

/// How many attempts a profile gets and the run's spend ceiling (Task 6.1).
#[derive(Clone, Copy, Debug)]
pub struct RetryConfig {
    /// Max attempts per profile before failover/stop (1 = no same-profile retry).
    pub max_retries_per_profile: u32,
    /// Cumulative USD ceiling for the whole request; `None` = uncapped.
    pub spend_cap_usd: Option<f64>,
}

/// Terminal outcome of a retried request across a profile and its fallbacks.
#[derive(Clone, Debug, PartialEq)]
pub enum RequestOutcome<T> {
    /// A profile produced a result. `profile_index` is 0 for the primary,
    /// 1.. for the ordered fallbacks actually rotated to.
    Ok {
        value: T,
        profile_index: usize,
        total_attempts: u32,
        spent_usd: f64,
    },
    /// The request stopped without a result, carrying a secret-free reason
    /// (`spend_cap`, or a `decide_retry` stop code).
    Stopped {
        reason: &'static str,
        total_attempts: u32,
        spent_usd: f64,
    },
}

/// Drive a model request across `num_profiles` (index 0 = primary, 1.. = ordered
/// fallbacks) with classified retry/failover and per-attempt cost accounting
/// (Task 6.1). `attempt(profile_index)` performs one provider call;
/// `cost_per_attempt_usd` is charged for every attempt (success or failure);
/// `backoff(ms)` awaits the `decide_retry` delay (a no-op in tests). Stops when:
/// the spend cap is reached (before over-spending), a non-retryable/non-failover
/// error classifies to `Stop`, or the fallbacks are exhausted. Pure over the
/// injected `attempt`/`backoff`, so it is ScriptedDriver-testable without a live
/// provider.
pub async fn request_with_retry<T, A, AFut, B, BFut>(
    num_profiles: usize,
    cfg: RetryConfig,
    cost_per_attempt_usd: f64,
    mut attempt: A,
    mut backoff: B,
) -> RequestOutcome<T>
where
    A: FnMut(usize) -> AFut,
    AFut: Future<Output = Result<T, crate::agent::provider::ProviderError>>,
    B: FnMut(u64) -> BFut,
    BFut: Future<Output = ()>,
{
    let mut profile = 0usize;
    let mut attempt_on_profile = 0u32;
    let mut total_attempts = 0u32;
    let mut spent = 0.0f64;
    loop {
        // Enforce the cap BEFORE spending on the next attempt, so the ceiling is
        // never overshot by more than one in-flight attempt.
        if over_spend_cap(spent, cfg.spend_cap_usd) {
            return RequestOutcome::Stopped {
                reason: "spend_cap",
                total_attempts,
                spent_usd: spent,
            };
        }
        attempt_on_profile += 1;
        total_attempts += 1;
        spent += cost_per_attempt_usd;
        match attempt(profile).await {
            Ok(value) => {
                return RequestOutcome::Ok {
                    value,
                    profile_index: profile,
                    total_attempts,
                    spent_usd: spent,
                }
            }
            Err(err) => {
                let has_fallback = profile + 1 < num_profiles;
                match decide_retry(
                    err,
                    attempt_on_profile,
                    cfg.max_retries_per_profile,
                    None,
                    has_fallback,
                ) {
                    RetryDecision::Retry { backoff_ms } => backoff(backoff_ms).await,
                    RetryDecision::Failover => {
                        profile += 1;
                        attempt_on_profile = 0;
                    }
                    RetryDecision::Stop { reason } => {
                        return RequestOutcome::Stopped {
                            reason,
                            total_attempts,
                            spent_usd: spent,
                        }
                    }
                }
            }
        }
    }
}
/// The vision-routing verdict for a turn that carries image attachments.
#[derive(Clone, Debug, PartialEq)]
pub enum VisionRoute {
    /// The configured model already reads images — no switch.
    KeepCurrent,
    /// Route THIS TURN ONLY to the named model (cheapest eligible).
    /// `(id, display name, $ per 1M output tokens)`.
    Route(String, String, f64),
    /// Vision models exist but none within the user's price cap.
    NoneAffordable,
    /// The configured model is not in the catalog — never route blind.
    CurrentUnknown,
}

/// Smallest context window an auto-routed vision model may have. The agent
/// system prompt + tool schemas + an image already reach tens of thousands
/// of tokens; a smaller window would trade one failure (can't see) for
/// another (context overflow mid-turn).
pub const MIN_ROUTE_CONTEXT: u64 = 32_000;
/// Pick a model for an image turn, cheapest first, never above the cap.
///
/// Contract (billing-safety):
/// - the current model keeps the turn whenever it can already see;
/// - a model missing from the catalog is NEVER auto-switched away from
///   (custom/self-hosted ids stay untouched);
/// - candidates MUST advertise vision AND native tool calling (the agent
///   loop always offers tools), MUST carry a parseable completion price at
///   or under `cap_per_mtok_out` (unknown price = ineligible), MUST NOT be
///   a `:free` variant (heavily rate-limited — they fail mid-run), and MUST
///   have a context window of at least [`MIN_ROUTE_CONTEXT`] tokens;
/// - ordering: completion price, then prompt price, then id (deterministic);
/// - `cap_per_mtok_out` ≤ 0 or non-finite disables routing entirely.
pub fn route_for_vision(
    catalog: &[fm_extract::OpenRouterModel],
    current_model: &str,
    cap_per_mtok_out: f64,
) -> VisionRoute {
    if !cap_per_mtok_out.is_finite() || cap_per_mtok_out <= 0.0 {
        return VisionRoute::NoneAffordable;
    }
    let Some(current) = catalog.iter().find(|m| m.id == current_model) else {
        return VisionRoute::CurrentUnknown;
    };
    if current.vision() {
        return VisionRoute::KeepCurrent;
    }
    let mut candidates: Vec<(&fm_extract::OpenRouterModel, f64, f64)> = catalog
        .iter()
        .filter(|m| {
            m.vision()
                && m.native_tools()
                && !m.id.ends_with(":free")
                && m.context_length.unwrap_or(0) >= MIN_ROUTE_CONTEXT
        })
        .filter_map(|m| {
            let out = m.completion_per_mtok()?;
            let inp = m.prompt_per_mtok()?;
            (out <= cap_per_mtok_out).then_some((m, out, inp))
        })
        .collect();
    if candidates.is_empty() {
        return VisionRoute::NoneAffordable;
    }
    candidates.sort_by(|a, b| {
        a.1.total_cmp(&b.1)
            .then(a.2.total_cmp(&b.2))
            .then(a.0.id.cmp(&b.0.id))
    });
    let (m, out, _) = candidates[0];
    let name = if m.name.trim().is_empty() {
        m.id.clone()
    } else {
        m.name.clone()
    };
    VisionRoute::Route(m.id.clone(), name, out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(base: &str, model: &str, cred: &str, tools: bool) -> ModelProfile {
        ModelProfile {
            provider_base: base.into(),
            model: model.into(),
            context_window: 128_000,
            native_tools: tools,
            structured_output: true,
            cost_per_mtok_in: None,
            cost_per_mtok_out: None,
            credential_ref: cred.into(),
        }
    }

    #[test]
    fn worker_and_verifier_default_to_orchestrator() {
        let orch = profile(
            "https://openrouter.ai/api/v1",
            "anthropic/claude-sonnet-4",
            "openrouter_api_key",
            true,
        );
        let roster = ModelRoster {
            orchestrator: orch.clone(),
            worker: None,
            verifier: None,
            fallbacks: vec![],
        };
        assert_eq!(roster.route(ModelRole::Worker), &orch);
        assert_eq!(roster.route(ModelRole::Verifier), &orch);
    }

    #[test]
    fn orchestrator_and_worker_coexist_without_credential_or_capability_leakage() {
        // OpenRouter orchestrator + a DeepSeek worker with a DISTINCT credential.
        let orch = profile(
            "https://openrouter.ai/api/v1",
            "anthropic/claude-sonnet-4",
            "openrouter_api_key",
            true,
        );
        let worker = profile(
            "https://api.deepseek.com/v1",
            "deepseek-chat",
            "worker_api_key",
            true,
        );
        let roster = ModelRoster {
            orchestrator: orch.clone(),
            worker: Some(worker.clone()),
            verifier: None,
            fallbacks: vec![],
        };
        let ro = roster.route(ModelRole::Orchestrator);
        let rw = roster.route(ModelRole::Worker);
        // Each role resolves its OWN provider, model, and credential ref.
        assert_eq!(ro.provider_base, "https://openrouter.ai/api/v1");
        assert_eq!(rw.provider_base, "https://api.deepseek.com/v1");
        assert_ne!(ro.credential_ref, rw.credential_ref);
        assert_ne!(ro.model, rw.model);
        // No secret is stored on the profile — only a credential-ref account name.
        assert_eq!(rw.credential_ref, "worker_api_key");
    }

    #[test]
    fn capability_validation_rejects_toolless_orchestrator() {
        let orch = profile("b", "m", "c", false); // no native tools
        let roster = ModelRoster {
            orchestrator: orch,
            worker: None,
            verifier: None,
            fallbacks: vec![],
        };
        assert!(roster.validate_role(ModelRole::Orchestrator).is_err());
    }

    #[test]
    fn fallback_chain_dedups_the_primary() {
        let orch = profile("b", "m", "c", true);
        let same = profile("b", "m", "c2", true);
        let other = profile("b2", "m2", "c3", true);
        let roster = ModelRoster {
            orchestrator: orch,
            worker: None,
            verifier: None,
            fallbacks: vec![same, other.clone()],
        };
        let chain = roster.fallback_chain(ModelRole::Orchestrator);
        // Primary + only the genuinely-different fallback (same (base,model) dropped).
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[1], &other);
    }

    #[test]
    fn from_settings_migrates_single_model_to_orchestrator() {
        let mut s = Settings {
            model: "x/y".into(),
            ..Default::default()
        };
        s.model_capability = Some(crate::commands::settings::ModelCapability {
            model_id: "x/y".into(),
            native_tools: true,
            strict_json: false,
            tested_at: String::new(),
        });
        let roster = ModelRoster::from_settings(&s);
        assert_eq!(roster.orchestrator.model, "x/y");
        assert!(roster.orchestrator.native_tools);
        assert_eq!(roster.orchestrator.credential_ref, "openrouter_api_key");
        // Unconfigured roles fall back to the orchestrator.
        assert_eq!(roster.route(ModelRole::Worker).model, "x/y");
    }

    #[test]
    fn retry_only_safe_categories_bounded() {
        use crate::agent::provider::ProviderError as E;
        // Retryable + under cap → Retry with backoff.
        assert!(matches!(
            decide_retry(E::RateLimit, 1, 3, None, false),
            RetryDecision::Retry { .. }
        ));
        // Honors server Retry-After.
        assert_eq!(
            decide_retry(E::Capacity, 1, 3, Some(2000), false),
            RetryDecision::Retry { backoff_ms: 2000 }
        );
        // Exhausted retries on a failover category with a fallback → Failover.
        assert_eq!(
            decide_retry(E::RateLimit, 3, 3, None, true),
            RetryDecision::Failover
        );
        // Auth is not retryable but is failover.
        assert_eq!(
            decide_retry(E::Auth, 1, 3, None, true),
            RetryDecision::Failover
        );
        // Auth with no fallback → Stop.
        assert_eq!(
            decide_retry(E::Auth, 1, 3, None, false),
            RetryDecision::Stop { reason: "auth" }
        );
        // Context overflow always stops here (compaction handles it upstream).
        assert_eq!(
            decide_retry(E::ContextOverflow, 1, 3, None, true),
            RetryDecision::Stop {
                reason: "context_overflow"
            }
        );
    }

    #[test]
    fn spend_cap_gates() {
        assert!(!over_spend_cap(1.0, None));
        assert!(!over_spend_cap(1.0, Some(2.0)));
        assert!(over_spend_cap(2.0, Some(2.0)));
        assert!(over_spend_cap(3.0, Some(2.0)));
    }

    // ---- Task 6.1: retry/failover/cost orchestration (ScriptedDriver-testable) ----

    use crate::agent::provider::ProviderError as PErr;

    fn cfg(max: u32, cap: Option<f64>) -> RetryConfig {
        RetryConfig {
            max_retries_per_profile: max,
            spend_cap_usd: cap,
        }
    }

    #[tokio::test]
    async fn retry_succeeds_on_first_attempt() {
        let out = request_with_retry(
            2,
            cfg(3, None),
            0.01,
            |_p| async { Ok::<u32, PErr>(7) },
            |_ms| async {},
        )
        .await;
        assert_eq!(
            out,
            RequestOutcome::Ok {
                value: 7,
                profile_index: 0,
                total_attempts: 1,
                spent_usd: 0.01,
            }
        );
    }

    #[tokio::test]
    async fn retryable_error_retries_same_profile_then_succeeds() {
        let mut n = 0u32;
        let mut backoffs = 0u32;
        let out = request_with_retry(
            2,
            cfg(3, None),
            0.01,
            |_p| {
                n += 1;
                let r = if n == 1 {
                    Err(PErr::Transport)
                } else {
                    Ok(1u32)
                };
                async move { r }
            },
            |_ms| {
                backoffs += 1;
                async {}
            },
        )
        .await;
        // Same profile, two attempts, one backoff between them.
        assert!(matches!(
            out,
            RequestOutcome::Ok {
                profile_index: 0,
                total_attempts: 2,
                ..
            }
        ));
        assert_eq!(backoffs, 1);
    }

    #[tokio::test]
    async fn failover_rotates_to_next_profile() {
        // Primary always fails with a failover-only error; fallback succeeds.
        let out = request_with_retry(
            2,
            cfg(3, None),
            0.02,
            |p| async move {
                if p == 0 {
                    Err(PErr::ToolIncompatible)
                } else {
                    Ok(9u32)
                }
            },
            |_ms| async {},
        )
        .await;
        assert!(matches!(
            out,
            RequestOutcome::Ok {
                value: 9,
                profile_index: 1,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn spend_cap_stops_before_overspending() {
        // Each attempt costs 0.6; cap 1.0 → after two charges (1.2) the third is
        // refused before it spends.
        let out = request_with_retry(
            1,
            cfg(9, Some(1.0)),
            0.6,
            |_p| async { Err::<u32, PErr>(PErr::Transport) },
            |_ms| async {},
        )
        .await;
        assert_eq!(
            out,
            RequestOutcome::Stopped {
                reason: "spend_cap",
                total_attempts: 2,
                spent_usd: 1.2,
            }
        );
    }

    #[tokio::test]
    async fn non_retryable_no_fallback_stops_with_reason() {
        let out = request_with_retry(
            1,
            cfg(3, None),
            0.0,
            |_p| async { Err::<u32, PErr>(PErr::Auth) },
            |_ms| async {},
        )
        .await;
        assert_eq!(
            out,
            RequestOutcome::Stopped {
                reason: "auth",
                total_attempts: 1,
                spent_usd: 0.0,
            }
        );
    }

    #[tokio::test]
    async fn exhausted_fallbacks_stop() {
        // Both profiles fail with a failover error and no same-profile retry.
        let out = request_with_retry(
            2,
            cfg(1, None),
            0.0,
            |_p| async { Err::<u32, PErr>(PErr::Capacity) },
            |_ms| async {},
        )
        .await;
        assert_eq!(
            out,
            RequestOutcome::Stopped {
                reason: "capacity",
                total_attempts: 2,
                spent_usd: 0.0,
            }
        );
    }
    // ── route_for_vision ────────────────────────────────────────────

    fn cat_model(
        id: &str,
        vision: bool,
        tools: bool,
        ctx: u64,
        prompt: &str,
        completion: &str,
    ) -> fm_extract::OpenRouterModel {
        let modalities = if vision {
            vec!["text", "image"]
        } else {
            vec!["text"]
        };
        let params = if tools { vec!["tools"] } else { vec![] };
        serde_json::from_value(serde_json::json!({
            "id": id,
            "name": id,
            "context_length": ctx,
            "pricing": { "prompt": prompt, "completion": completion },
            "supported_parameters": params,
            "architecture": { "input_modalities": modalities },
        }))
        .unwrap()
    }

    #[test]
    fn vision_keeps_a_model_that_already_sees() {
        let cat = vec![cat_model("a/sees", true, true, 128_000, "0.000001", "0.000002")];
        assert_eq!(route_for_vision(&cat, "a/sees", 5.0), VisionRoute::KeepCurrent);
    }

    #[test]
    fn vision_never_routes_away_from_an_unknown_model() {
        let cat = vec![cat_model("a/sees", true, true, 128_000, "0.000001", "0.000002")];
        assert_eq!(
            route_for_vision(&cat, "custom/self-hosted", 5.0),
            VisionRoute::CurrentUnknown
        );
    }

    #[test]
    fn vision_routes_cheapest_eligible_under_cap() {
        let cat = vec![
            cat_model("t/blind", false, true, 128_000, "0.0000004", "0.0000016"),
            // $12/M out — above the $5 cap.
            cat_model("v/pricey", true, true, 200_000, "0.000003", "0.000012"),
            // $2.40/M out — eligible, but pricier than v/cheap.
            cat_model("v/mid", true, true, 128_000, "0.0000006", "0.0000024"),
            // $0.80/M out — the winner.
            cat_model("v/cheap", true, true, 64_000, "0.0000002", "0.0000008"),
            // Cheaper than v/cheap but rate-limited free variant: excluded.
            cat_model("v/gratis:free", true, true, 128_000, "0", "0"),
            // Cheaper but no tool support: the agent loop can't use it.
            cat_model("v/no-tools", true, false, 128_000, "0.0000001", "0.0000004"),
            // Cheaper but a tiny window: overflows mid-turn.
            cat_model("v/tiny-ctx", true, true, 8_000, "0.0000001", "0.0000004"),
        ];
        match route_for_vision(&cat, "t/blind", 5.0) {
            VisionRoute::Route(id, name, out) => {
                assert_eq!(id, "v/cheap");
                assert_eq!(name, "v/cheap");
                assert!((out - 0.8).abs() < 1e-9, "out price {out}");
            }
            other => panic!("expected Route(v/cheap), got {other:?}"),
        }
    }

    #[test]
    fn vision_cap_excludes_everything_or_disables() {
        let cat = vec![
            cat_model("t/blind", false, true, 128_000, "0.0000004", "0.0000016"),
            cat_model("v/pricey", true, true, 200_000, "0.000003", "0.000012"),
        ];
        // Cap below every candidate → NoneAffordable, never a silent overshoot.
        assert_eq!(route_for_vision(&cat, "t/blind", 1.0), VisionRoute::NoneAffordable);
        // Cap 0 / NaN → routing disabled outright.
        assert_eq!(route_for_vision(&cat, "t/blind", 0.0), VisionRoute::NoneAffordable);
        assert_eq!(
            route_for_vision(&cat, "t/blind", f64::NAN),
            VisionRoute::NoneAffordable
        );
    }

    #[test]
    fn vision_unknown_price_is_ineligible() {
        let cat = vec![
            cat_model("t/blind", false, true, 128_000, "0.0000004", "0.0000016"),
            // No parseable price — must NOT be routed to, whatever the cap.
            cat_model("v/mystery", true, true, 128_000, "", ""),
        ];
        assert_eq!(route_for_vision(&cat, "t/blind", 100.0), VisionRoute::NoneAffordable);
    }
}
