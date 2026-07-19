// labels.mjs — shared human copy for tool activity (warm colleague voice).
// Never surface snake_case tool ids, API enums, or "Running foo_bar…" in the UI.

/**
 * @typedef {Object} ToolStory
 * @property {string} running  - progressive label while the tool is live
 * @property {string} done     - past-tense outcome when it succeeds
 * @property {string} failed   - soft failure when it errors
 * @property {string} [verb]   - short verb for wrapping a bare detail/query
 */

/** @type {Record<string, ToolStory>} */
export const TOOL_STORY = {
  research: {
    running: "Digging into the research…",
    done: "Finished the research",
    failed: "Couldn't finish the research",
    verb: "Researching",
  },
  analyze_pdf: {
    running: "Reading through the PDF…",
    done: "Finished the PDF",
    failed: "Couldn't read the PDF",
    verb: "Reading",
  },
  build_model: {
    running: "Building the model…",
    done: "Built the model",
    failed: "Couldn't build the model",
    verb: "Building",
  },
  draft_memo: {
    running: "Writing it up…",
    done: "Drafted the memo",
    failed: "Couldn't finish the draft",
    verb: "Drafting",
  },
  benchmark_peers: {
    running: "Comparing peers…",
    done: "Compared the peers",
    failed: "Couldn't compare peers",
    verb: "Comparing",
  },
  get_financials: {
    running: "Pulling the financials…",
    done: "Pulled the financials",
    failed: "Couldn't pull the financials",
    verb: "Pulling financials for",
  },
  get_quote: {
    running: "Checking the latest price…",
    done: "Got the latest price",
    failed: "Couldn't get the price",
    verb: "Checking price for",
  },
  list_filings: {
    running: "Finding filings…",
    done: "Found the filings",
    failed: "Couldn't find filings",
    verb: "Finding filings for",
  },
  read_filing: {
    running: "Reading the filing…",
    done: "Read the filing",
    failed: "Couldn't read the filing",
    verb: "Reading",
  },
  web_search: {
    running: "Searching the web…",
    done: "Finished the search",
    failed: "Couldn't search the web",
    verb: "Searching",
  },
  read_page: {
    running: "Reading the page…",
    done: "Read the page",
    failed: "Couldn't read the page",
    verb: "Reading",
  },
  research_deal: {
    running: "Looking into the deal…",
    done: "Finished deal research",
    failed: "Couldn't research the deal",
    verb: "Researching",
  },
  get_news: {
    running: "Scanning the news…",
    done: "Pulled the news",
    failed: "Couldn't pull the news",
    verb: "Scanning news for",
  },
  use_skill: {
    running: "Using a saved playbook…",
    done: "Used a saved playbook",
    failed: "Couldn't use the playbook",
    verb: "Using",
  },
};

/** Strip mechanical prefixes and turn snake_case into plain words. */
export function humanizeToolId(name) {
  let s = String(name || "")
    .trim()
    .replace(/_/g, " ");
  s = s.replace(/^(get|list|read|fetch|run|use|do)\s+/i, "").trim();
  return s || "this next step";
}

function looksLikeSentence(detail) {
  const d = String(detail || "").trim();
  if (!d) return false;
  if (/[….!?]$/.test(d)) return true;
  if (d.length > 42) return true;
  return /\s/.test(d) && d.split(/\s+/).length >= 4;
}

/**
 * Live progress / thinking-step label.
 * Prefer a backend detail when it's already human; otherwise wrap a bare query.
 * Never returns Running snake_case…
 */
export function toolRunningLabel(name, detail) {
  const d = detail && String(detail).trim();
  const story = TOOL_STORY[name];
  if (d) {
    if (looksLikeSentence(d)) return d;
    const verb = (story && story.verb) || "Looking up";
    return `${verb} ${d}…`;
  }
  if (story) return story.running;
  return `Checking ${humanizeToolId(name)}…`;
}

/** Past-tense success label for a completed step. */
export function toolDoneLabel(name, detail) {
  const story = TOOL_STORY[name];
  if (story) return story.done;
  const d = detail && String(detail).trim();
  if (d && !looksLikeSentence(d)) return `Finished checking ${d}`;
  return `Finished checking ${humanizeToolId(name)}`;
}

/** Soft failure label. Never "failed" alone. */
export function toolFailedLabel(name) {
  const story = TOOL_STORY[name];
  if (story) return story.failed;
  return `Couldn't finish checking ${humanizeToolId(name)}`;
}

/** Short noun-ish name for activity rows (not a full sentence). */
export function toolShortName(name, label) {
  if (label && String(label).trim()) return String(label).trim();
  switch (name) {
    case "get_financials":
      return "Financials";
    case "get_quote":
      return "Price quote";
    case "benchmark_peers":
      return "Peer comparison";
    case "build_model":
      return "Model build";
    case "web_search":
      return "Web search";
    case "read_page":
      return "Page read";
    case "list_filings":
      return "Filings list";
    case "read_filing":
      return "Filing";
    case "analyze_pdf":
      return "PDF review";
    case "research":
    case "research_deal":
      return "Research";
    case "get_news":
      return "News";
    case "use_skill":
      return "Playbook";
    default:
      break;
  }
  const h = humanizeToolId(name);
  return h.replace(/\b\w/g, (c) => c.toUpperCase());
}

export function fanoutRunningLabel(count) {
  const n = Number(count) || 2;
  return n === 1
    ? "Looking into one more thing…"
    : `Looking at ${n} things together…`;
}

export function fanoutDoneLabel(count) {
  const n = Number(count) || 2;
  return n === 1 ? "Finished that check" : `Finished those ${n} checks`;
}

/** Agent phase → polite progress copy. */
export function agentPhaseLabel(phase) {
  switch (phase) {
    case "planning":
      return "Mapping out the approach…";
    case "synthesizing":
      return "Writing this up…";
    case "verifying":
      return "Double-checking the figures…";
    default:
      return null;
  }
}


// ── Card / result copy (warm colleague, no schema voice) ─────────────

export function confidenceLabel(level) {
  const k = String(level || "").toLowerCase();
  switch (k) {
    case "high":
      return "High confidence";
    case "medium":
    case "med":
      return "Fairly confident";
    case "low":
      return "Still tentative";
    case "":
      return "";
    default:
      return "Confidence noted";
  }
}

export function verifyStatusLabel(status) {
  switch (String(status || "")) {
    case "verified":
      return "Figures checked";
    case "verified_with_warnings":
      return "Checked, with notes";
    case "partial_unverified":
    default:
      return "Partly checked";
  }
}

export function pageStatusLabel(status) {
  const k = String(status || "ok").toLowerCase();
  switch (k) {
    case "ok":
    case "read":
    case "ready":
      return "Ready to read";
    case "thin":
      return "Sparse page";
    case "blocked":
      return "Couldn't open";
    case "error":
    case "failed":
      return "Had trouble loading";
    default:
      return humanizeToolId(k);
  }
}

export function sourceStatusLabel(status) {
  const k = String(status || "").toLowerCase();
  switch (k) {
    case "ok":
    case "read":
      return "Read";
    case "thin":
      return "Not much there";
    case "blocked":
      return "Site blocked us";
    case "skipped":
      return "Passed over";
    case "error":
    case "failed":
      return "Couldn't read";
    case "":
      return "";
    default:
      return humanizeToolId(k);
  }
}

export function sourceKindLabel(kind) {
  const k = String(kind || "").toLowerCase();
  switch (k) {
    case "regulatory":
      return "Regulator";
    case "company":
      return "Company source";
    case "primary":
      return "Company statement";
    case "newswire":
      return "Press";
    case "secondary":
      return "Web";
    case "filing":
      return "Filing";
    case "news":
      return "News";
    case "page":
    case "web":
      return "Web";
    case "transcript":
      return "Transcript";
    case "press":
      return "Press";
    case "ir":
      return "Investor relations";
    case "":
      return "";
    default:
      return humanizeToolId(k);
  }
}

export function dealSufficiencyLabel(sufficient) {
  return sufficient ? "Looks complete" : "Still missing pieces";
}

const FACT_KEYS = {
  deal_value: "Deal value",
  enterprise_value: "Enterprise value",
  equity_value: "Equity value",
  price_per_share: "Price per share",
  premium_pct: "Premium",
  consideration: "Consideration",
  announce_date: "Announced",
  close_date: "Expected close",
  status: "Status",
  acquirer: "Acquirer",
  target: "Target",
  multiple: "Multiple",
  syn_ergies: "Synergies",
  synergies: "Synergies",
};

export function factKeyLabel(key) {
  const k = String(key || "");
  if (FACT_KEYS[k]) return FACT_KEYS[k];
  return humanizeToolId(k)
    .split(" ")
    .map((w) => (w ? w[0].toUpperCase() + w.slice(1) : w))
    .join(" ");
}

export function formatFactValue(v) {
  if (v == null || v === "") return "";
  if (typeof v === "number") return String(v);
  if (typeof v === "string") return v;
  if (Array.isArray(v)) return v.map((x) => formatFactValue(x)).filter(Boolean).join(", ");
  if (typeof v === "object") {
    // Prefer readable primitives over JSON dumps.
    if (v.label != null) return String(v.label);
    if (v.value != null && (typeof v.value !== "object")) return String(v.value);
    if (v.amount != null) {
      const unit = v.unit || v.currency || "";
      return unit ? `${v.amount} ${unit}`.trim() : String(v.amount);
    }
    if (v.text != null) return String(v.text);
    const parts = Object.entries(v)
      .filter(([, val]) => val != null && typeof val !== "object")
      .map(([key, val]) => `${factKeyLabel(key)}: ${val}`);
    return parts.length ? parts.join("; ") : "See details";
  }
  return String(v);
}

/** Inline citation marker — compact number like Grok, never schema ids. */
export function citeChipLabel(src, sourceId, index) {
  if (index != null && Number.isFinite(Number(index)) && Number(index) >= 0) {
    return String(Number(index) + 1);
  }
  const id = String(sourceId || "");
  if (/^s?\d+$/i.test(id)) {
    const n = id.replace(/^s/i, "");
    return n || "1";
  }
  return "1";
}

/** Publisher/domain for source cards and tooltips. */
export function sourcePublisherLabel(src, url) {
  const domain = (src && (src.domain || "")) || "";
  if (domain) return domain.replace(/^www\./i, "");
  const fromUrl = String(url || "");
  if (fromUrl) {
    try {
      return new URL(fromUrl).hostname.replace(/^www\./i, "");
    } catch (_) {}
  }
  return "";
}

/** Card title: prefer human title, fall back to publisher. */
export function sourceCardTitle(src, url) {
  const title = String((src && src.title) || "").trim();
  if (title) return title;
  return sourcePublisherLabel(src, url) || "Source";
}

/** One-letter avatar when favicons are unavailable (CSP / offline). */
export function sourceAvatarLetter(publisher) {
  const d = String(publisher || "S").replace(/^www\./i, "").trim();
  const ch = d.charAt(0);
  return ch ? ch.toUpperCase() : "S";
}

export function sourceRowMeta(status, kind) {
  const a = sourceStatusLabel(status);
  const b = sourceKindLabel(kind);
  if (a && b) return `${a} · ${b}`;
  return a || b || "";
}

export function softErrorMessage(fallback) {
  return fallback || "Couldn't finish that just now.";
}

// ── Approval vocabulary (one voice across chat / parts / activity) ──

export function approvalHead(risk) {
  return risk
    ? "Before I continue, I need your OK on this write."
    : "Before I continue, I need your OK.";
}

export function approvalApproveLabel() {
  return "Go ahead";
}

export function approvalDenyLabel() {
  return "Not this time";
}

export function approvalNewVersionLabel() {
  return "Save as a new version";
}

/** Quiet mission meta line: workflow · plan · verify. */
export function missionMetaLine({ workflow, planDone, planTotal, verify } = {}) {
  const parts = [];
  if (workflow) parts.push(workflow);
  const total = Number(planTotal) || 0;
  if (total > 0) {
    parts.push((Number(planDone) || 0) + " of " + total + " done");
  }
  const v = verifyStatusLabel(verify);
  if (verify && v) parts.push(v);
  return parts.join(" · ");
}

// ── filings: human names for forms and items ────────────────────────
// SEC form codes and item numbers are filing jargon; the card wears the
// plain-English name next to the code so a non-expert knows what they got.

const FILING_FORM_NAMES = {
  "10-K": "Annual report",
  "10-Q": "Quarterly report",
  "8-K": "Current report",
  "DEF 14A": "Proxy statement",
  "20-F": "Annual report (foreign issuer)",
  "S-1": "IPO registration",
  "424B4": "IPO prospectus",
};

const FILING_ITEM_NAMES = {
  "10-K": {
    1: "Business",
    "1A": "Risk factors",
    "1B": "Unresolved staff comments",
    "1C": "Cybersecurity",
    2: "Properties",
    3: "Legal proceedings",
    5: "Market for the stock",
    7: "Management's discussion (MD&A)",
    "7A": "Market risk",
    8: "Financial statements",
    "9A": "Controls and procedures",
    10: "Directors and governance",
    11: "Executive compensation",
    15: "Exhibits",
  },
  "10-Q": {
    1: "Financial statements",
    2: "Management's discussion (MD&A)",
    3: "Market risk",
    4: "Controls and procedures",
  },
  "8-K": {
    1: "Business and operations",
    2: "Financial information",
    3: "Securities and trading",
    4: "Accountant and financial-statement matters",
    5: "Governance and management",
    6: "Asset-backed securities",
    7: "Regulation FD disclosure",
    8: "Other events",
    9: "Financial statements and exhibits",
  },
};

/** "10-K" → "Annual report"; unknown forms fall back to the code itself. */
export function filingFormLabel(form) {
  const f = String(form || "").trim().toUpperCase();
  return FILING_FORM_NAMES[f] || f;
}

/**
 * "Item 2" → "Item 2 · Financial information" (form-aware; 8-K sub-items like
 * "2.02" resolve on their major number). Unknown items stay "Item <id>".
 */
export function filingItemLabel(form, id) {
  const f = String(form || "").trim().toUpperCase();
  const raw = String(id == null ? "" : id).trim().toUpperCase();
  if (!raw) return "";
  const table = FILING_ITEM_NAMES[f] || {};
  const name = table[raw] || table[raw.split(".")[0]];
  return name ? `Item ${raw} · ${name}` : `Item ${raw}`;
}

/**
 * Warm label for a schedule's coarse due semantics. Event-anchored phrases
 * are honestly approximate — the backend schedules a concrete date.
 */
export function scheduleDueLabel(due) {
  switch (String(due || "")) {
    case "tomorrow":
      return "tomorrow";
    case "next_week":
      return "in a week";
    case "next_quarter":
      return "next quarter (about three months out)";
    case "after_next_earnings":
      return "after the next earnings report (about five weeks)";
    default:
      return "in about a week";
  }
}

/** Memo kinds in plain words. */
export function memoKindLabel(kind) {
  switch (String(kind || "")) {
    case "earnings_note":
      return "Earnings note";
    case "company_profile":
      return "Company profile";
    case "deal_summary":
      return "Deal summary";
    case "comps_note":
      return "Comps note";
    default:
      return "Memo";
  }
}

/** After an evidence-gathering turn, which memo (if any) is worth offering?
 * Returns null when there's nothing to draft from — or a memo already exists.
 * Priority mirrors specificity: a deal beats comps beats earnings beats profile.
 */
export function draftOfferForCards(types) {
  const t = new Set(types || []);
  if (t.has("memo")) return null;
  if (t.has("deal"))
    return {
      kind: "deal_summary",
      prompt: "Draft the deal summary",
      text: "Want the write-up? I can draft a deal summary from what we just gathered.",
    };
  if (t.has("benchmark"))
    return {
      kind: "comps_note",
      prompt: "Draft the comps note",
      text: "Want the write-up? I can draft a comps note from this peer comparison.",
    };
  if (t.has("financials"))
    return {
      kind: "earnings_note",
      prompt: "Draft the earnings note",
      text: "Want the write-up? I can draft an earnings note from these figures.",
    };
  if (t.has("research_answer"))
    return {
      kind: "company_profile",
      prompt: "Draft the company profile",
      text: "Want the write-up? I can draft a company profile from this research.",
    };
  return null;
}

