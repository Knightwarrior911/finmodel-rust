// cards.mjs — inline result cards for chat tool outputs. Each card is the only
// card treatment in the UI (they are interactive). renderCard(card) → element.

import {
  call,
  escapeHtml,
  openExternal,
  openPath,
  domainOf,
  fmtNum,
  fmtPct,
  fmtPrice,
  fmtMoneyM,
  copyToClipboard,
  flashBtn,
} from "./core.mjs";
import { openReader } from "./reader.mjs";
import { openAnalyst } from "./analyst.mjs";

function parentDir(p) {
  return String(p || "").replace(/[\\/][^\\/]+$/, "");
}

function cardShell(kind, inner) {
  const el = document.createElement("div");
  el.className = `card card-${kind}`;
  el.innerHTML = inner;
  return el;
}

// ── model ───────────────────────────────────────────────────────────
function valuationStrip(v) {
  if (!v || !v.has_dcf) return "";
  if (v.current_price && v.price_per_share != null) {
    const up = v.upside_pct;
    const cls = up == null ? "" : up >= 0 ? "up" : "down";
    return `<div class="val-strip">
      <span class="val-item"><span class="val-k">Implied</span><span class="val-v num">${escapeHtml(fmtPrice(v.price_per_share))}</span></span>
      <span class="val-item"><span class="val-k">Current</span><span class="val-v num">${escapeHtml(fmtPrice(v.current_price))}</span></span>
      <span class="val-item ${cls}"><span class="val-k">Upside</span><span class="val-v num">${escapeHtml(fmtPct(up))}</span></span>
      <span class="val-item"><span class="val-k">WACC</span><span class="val-v num">${escapeHtml(fmtPct(v.wacc))}</span></span>
      <span class="val-item"><span class="val-k">EV</span><span class="val-v num">${escapeHtml(fmtMoneyM(v.ev))}</span></span>
      ${v.method ? `<span class="val-method">${escapeHtml(v.method)}</span>` : ""}
    </div>`;
  }
  return `<p class="card-note">Add a share price for DCF upside.</p>`;
}

function renderModel(card) {
  const v = card.valuation || {};
  const comps = card.comps;
  const compsNote =
    comps && comps.count != null
      ? `<p class="card-note">Comps: ${comps.count} peer${comps.count === 1 ? "" : "s"}${
          comps.excluded && comps.excluded.length ? ` (${comps.excluded.length} excluded)` : ""
        }</p>`
      : "";
  const caseTag =
    card.case && card.case !== "base"
      ? `<span class="card-tag">${escapeHtml(card.case === "upside" ? "Upside case" : "Downside case")}</span>`
      : "";
  const inner = `
    <div class="card-head">
      <span class="card-title num">${escapeHtml(card.ticker || "")}</span>
      <span class="card-sub">${escapeHtml(card.currency || "")} · model</span>
      ${caseTag}
    </div>
    ${valuationStrip(v)}
    ${compsNote}
    <div class="card-actions">
      <button type="button" class="btn-primary" data-open-excel="${escapeHtml(card.xlsx_path || "")}">Open in Excel</button>
      ${card.pptx_path ? `<button type="button" class="btn-ghost" data-open-excel="${escapeHtml(card.pptx_path)}">Open deck</button>` : ""}
      <button type="button" class="btn-ghost" data-show-folder="${escapeHtml(card.xlsx_path || "")}">Show in folder</button>
      <button type="button" class="btn-ghost" data-analyst="1">Analyst tools</button>
    </div>`;
  return cardShell("model", inner);
}

// ── benchmark ───────────────────────────────────────────────────────
const BENCH_FMT = {
  fiscal_year: (v) => (v == null ? "—" : String(v)),
  revenue_m: fmtNum,
  ebitda_m: fmtNum,
  net_income_m: fmtNum,
  ebitda_margin: fmtPct,
  net_margin: fmtPct,
  roe: fmtPct,
  net_debt_to_ebitda: (v) => (v == null ? "—" : Number(v).toFixed(1) + "x"),
};

function renderBenchmark(card) {
  const headers = card.headers || [];
  const rows = card.rows || [];
  const thead = `<tr>${headers.map((h) => `<th scope="col">${escapeHtml(h.label)}</th>`).join("")}</tr>`;
  const tbody = rows
    .map((r) => {
      const cells = headers
        .map((h) => {
          const raw = r[h.key];
          const val = h.key === "ticker" ? escapeHtml(raw || "") : escapeHtml((BENCH_FMT[h.key] || String)(raw));
          const numCls = h.key === "ticker" ? "" : " class=\"num\"";
          return `<td${numCls}>${val}</td>`;
        })
        .join("");
      return `<tr>${cells}</tr>`;
    })
    .join("");
  const failed = (card.failed || [])
    .map((f) => `${escapeHtml(f.ticker)} (${escapeHtml(f.why)})`)
    .join(", ");
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(card.title || "Peer benchmark")}</span>
    </div>
    <div class="card-table-scroll"><div class="card-table-wrap"><table class="card-table"><thead>${thead}</thead><tbody>${tbody}</tbody></table></div></div>
    ${failed ? `<p class="card-note warn">Not fetched: ${failed}</p>` : ""}
    <div class="card-actions">
      <button type="button" class="btn-primary" data-open-excel="${escapeHtml(card.xlsx_path || "")}">Open in Excel</button>
      ${card.csv_path ? `<button type="button" class="btn-ghost" data-open-excel="${escapeHtml(card.csv_path)}">Open CSV</button>` : ""}
      ${card.pptx_path ? `<button type="button" class="btn-ghost" data-open-excel="${escapeHtml(card.pptx_path)}">Open deck</button>` : ""}
      <button type="button" class="btn-ghost" data-copy-table>Copy table</button>
    </div>`;
  const el = cardShell("benchmark", inner);
  const tsv = [
    headers.map((h) => h.label).join("\t"),
    ...rows.map((r) =>
      headers
        .map((h) => (h.key === "ticker" ? r[h.key] || "" : (BENCH_FMT[h.key] || String)(r[h.key])))
        .join("\t")
    ),
  ].join("\n");
  const copyBtn = el.querySelector("[data-copy-table]");
  if (copyBtn) {
    copyBtn.addEventListener("click", (e) => {
      e.stopPropagation();
      copyToClipboard(tsv);
      flashBtn(copyBtn, "Copied");
    });
  }
  return el;
}

// ── search ──────────────────────────────────────────────────────────
function renderSearch(card) {
  const hits = card.hits || [];
  const rows = hits
    .map(
      (h) => `<li class="hit-row" data-reader-url="${escapeHtml(h.url)}" data-reader-title="${escapeHtml(
        h.title || ""
      )}" tabindex="0" role="button">
        <div class="hit-main">
          <span class="hit-title">${escapeHtml(h.title || domainOf(h.url))}</span>
          <span class="hit-domain num">${escapeHtml(domainOf(h.url))}</span>
        </div>
        ${h.snippet ? `<p class="hit-snippet">${escapeHtml(h.snippet)}</p>` : ""}
        <button type="button" class="btn-ghost hit-open" data-url="${escapeHtml(h.url)}">Open ↗</button>
      </li>`
    )
    .join("");
  const inner = `
    <div class="card-head"><span class="card-title">Search · ${escapeHtml(card.query || "")}</span></div>
    <ul class="hit-list">${rows || '<li class="card-note">No results.</li>'}</ul>`;
  return cardShell("search", inner);
}

// ── page ────────────────────────────────────────────────────────────
function renderPage(card) {
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(card.title || domainOf(card.url))}</span>
      <span class="card-sub num">${escapeHtml(domainOf(card.url))} · ${escapeHtml(card.status || "ok")}</span>
    </div>
    <div class="card-actions">
      <button type="button" class="btn-primary" data-reader-url="${escapeHtml(card.url)}" data-reader-title="${escapeHtml(card.title || "")}">Open in reader</button>
      <button type="button" class="btn-ghost" data-url="${escapeHtml(card.url)}">Open ↗</button>
    </div>`;
  return cardShell("page", inner);
}

// ── news ────────────────────────────────────────────────────────────
function renderNews(card) {
  const items = card.items || [];
  const rows = items
    .map(
      (n) => `<li class="news-row" data-url="${escapeHtml(n.url)}" role="button" tabindex="0">
        <span class="news-title">${escapeHtml(n.title)}</span>
        <span class="news-src num">${escapeHtml(n.source || "")}${n.published ? " · " + escapeHtml(n.published) : ""}</span>
      </li>`
    )
    .join("");
  const inner = `
    <div class="card-head"><span class="card-title">News · ${escapeHtml(card.query || "")}</span></div>
    <ul class="news-list">${rows || '<li class="card-note">No headlines.</li>'}</ul>`;
  return cardShell("news", inner);
}

// ── deal ────────────────────────────────────────────────────────────
function renderDeal(card) {
  const s = card.summary || {};
  const facts = Object.entries(s)
    .filter(([, v]) => v != null && v !== "")
    .map(
      ([k, v]) =>
        `<div class="fact"><span class="fact-k">${escapeHtml(k.replace(/_/g, " "))}</span><span class="fact-v">${escapeHtml(
          typeof v === "object" ? JSON.stringify(v) : String(v)
        )}</span></div>`
    )
    .join("");
  const sources = (card.sources_read || [])
    .map((u) => `<li><a href="#" class="md-link" data-url="${escapeHtml(u)}">${escapeHtml(domainOf(u))}</a></li>`)
    .join("");
  const head =
    [card.acquirer, card.target].filter(Boolean).join(" / ") || "Deal research";
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(head)}</span>
      <span class="card-sub">${card.sufficient ? "sufficient" : "partial"}</span>
    </div>
    <div class="fact-grid">${facts || '<p class="card-note">No deal facts extracted.</p>'}</div>
    ${sources ? `<div class="card-sources"><span class="card-sub">Sources read</span><ul>${sources}</ul></div>` : ""}`;
  return cardShell("deal", inner);
}

// ── quote ───────────────────────────────────────────────────────────
function renderQuote(card) {
  const inner = `
    <div class="card-head"><span class="card-title num">${escapeHtml(card.ticker || "")}</span></div>
    <div class="quote-line">
      <span class="quote-price num">${escapeHtml(fmtPrice(card.price))}</span>
      <span class="quote-ccy">${escapeHtml(card.currency || "")}</span>
      ${
        card.week52_low != null && card.week52_high != null
          ? `<span class="quote-range num">52w ${escapeHtml(fmtPrice(card.week52_low))} – ${escapeHtml(fmtPrice(card.week52_high))}</span>`
          : ""
      }
    </div>`;
  return cardShell("quote", inner);
}

// ── filings ─────────────────────────────────────────────────────────
function renderFilings(card) {
  const rows = (card.rows || [])
    .map(
      (f) => `<tr class="filing-row" data-url="${escapeHtml(f.url)}" role="button" tabindex="0">
        <td class="num">${escapeHtml(f.form_type || "")}</td>
        <td class="num">${escapeHtml(f.filing_date || "")}</td>
        <td class="num">${escapeHtml(f.fiscal_period_end || "")}</td>
      </tr>`
    )
    .join("");
  const inner = `
    <div class="card-head"><span class="card-title num">${escapeHtml(card.ticker || "")}</span><span class="card-sub">filings</span></div>
    <div class="card-table-wrap"><table class="card-table"><thead><tr><th scope="col">Form</th><th scope="col">Filed</th><th scope="col">Period end</th></tr></thead><tbody>${
      rows || '<tr><td colspan="3" class="card-note">No filings.</td></tr>'
    }</tbody></table></div>`;
  return cardShell("filings", inner);
}

// ── filing_doc (10-K/10-Q reader) ───────────────────────────────────
function renderFilingDoc(card) {
  const items = card.items || [];
  const chips = items.map((id) => `<span class="filing-item-chip num">Item ${escapeHtml(id)}</span>`).join("");
  const sub = [
    escapeHtml(card.form || ""),
    card.item ? `Item ${escapeHtml(card.item)}` : null,
    card.filing_date ? escapeHtml(card.filing_date) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  const inner = `
    <div class="card-head">
      <span class="card-title num">${escapeHtml(card.ticker || "")}</span>
      <span class="card-sub">${sub}</span>
    </div>
    ${chips ? `<div class="filing-items">${chips}</div>` : ""}
    ${card.chars ? `<p class="card-note">${escapeHtml(String(card.chars))} characters extracted.</p>` : ""}
    <div class="card-actions">
      <button type="button" class="btn-ghost" data-url="${escapeHtml(card.url || "")}">Open in browser ↗</button>
    </div>`;
  return cardShell("filing_doc", inner);
}

// ── assumptions (interactive build grid) ────────────────────────────
function renderAssumptions(card) {
  const proj = card.proj_periods || [];
  const labels = card.labels || {};
  const drivers = card.drivers || {};
  const keys = Object.keys(labels);
  const cols = ["Driver", ...proj];
  const thead = `<tr>${cols.map((c) => `<th scope="col"${c === "Driver" ? "" : ' class="num"'}>${escapeHtml(c)}</th>`).join("")}</tr>`;
  const body = keys
    .map((key) => {
      const vals = drivers[key] || [];
      const cells = proj
        .map((_, i) => {
          const v = vals[i];
          const s = v == null ? "" : String(Math.round(v * 1e6) / 1e6);
          return `<td><input class="num" type="number" step="0.0001" data-orig="${escapeHtml(s)}" value="${escapeHtml(s)}"></td>`;
        })
        .join("");
      return `<tr data-key="${escapeHtml(key)}"><td class="lbl">${escapeHtml(labels[key] || key)}</td>${cells}</tr>`;
    })
    .join("");
  const inner = `
    <div class="card-head">
      <span class="card-title num">${escapeHtml(card.ticker || "")}</span>
      <span class="card-sub">${escapeHtml(card.currency || "")} · assumptions</span>
    </div>
    <p class="card-note">Edit any projected driver, then build. Blank cells keep the derived value.</p>
    <div class="card-table-wrap"><table class="card-table assumptions-table"><thead>${thead}</thead><tbody>${body}</tbody></table></div>
    <div class="card-actions">
      <button type="button" class="btn-primary" data-build-assumptions="${escapeHtml(card.session_id || "")}">Build with these assumptions</button>
      <span class="assumptions-status" hidden></span>
    </div>`;
  const el = cardShell("assumptions", inner);
  el.querySelectorAll("input").forEach((inp) => {
    inp.addEventListener("input", () => {
      inp.classList.toggle("edited", inp.value.trim() !== (inp.dataset.orig || ""));
    });
  });
  return el;
}

function collectOverrides(cardEl) {
  const overrides = [];
  cardEl.querySelectorAll("tbody tr").forEach((tr) => {
    const values = Array.from(tr.querySelectorAll("input")).map((inp) => {
      if (!inp.classList.contains("edited") || inp.value.trim() === "") return null;
      const n = Number(inp.value.trim());
      return isFinite(n) ? n : null;
    });
    if (values.some((v) => v != null)) overrides.push({ key: tr.dataset.key, values });
  });
  return overrides;
}

// ── research (cited answer + source digest) ─────────────────────────
// All source-derived strings pass through escapeHtml (the established XSS
// defense); clickable URLs come ONLY from the trusted ledger and must be
// http(s). Untrusted model text never becomes a URL or raw HTML.
function safeHttpUrl(u) {
  const s = String(u || "");
  return /^https?:\/\//i.test(s) ? s : "";
}

function citeRefs(citations, srcById) {
  return (citations || [])
    .map((c) => {
      const src = srcById[c.source_id] || {};
      const url = safeHttpUrl(src.final_url || src.requested_url || "");
      const attrs = url ? ` data-url="${escapeHtml(url)}"` : "";
      return `<button type="button" class="cite-ref"${attrs} title="${escapeHtml(
        c.quote || ""
      )}" aria-label="Source ${escapeHtml(c.source_id || "")}: ${escapeHtml(
        c.quote || ""
      )}">[${escapeHtml(c.source_id || "")}]</button>`;
    })
    .join("");
}

function citedPara(p, srcById) {
  return `<p class="cited-para">${escapeHtml(p.text || "")} ${citeRefs(p.citations, srcById)}</p>`;
}

function renderResearchAnswer(card) {
  const a = card.answer || {};
  const srcById = {};
  for (const s of a.sources || []) srcById[s.id] = s;
  const sections = (a.sections || [])
    .map(
      (s) =>
        `<section class="answer-section"><h4 class="answer-heading">${escapeHtml(
          s.heading || ""
        )}</h4>${(s.paragraphs || []).map((p) => citedPara(p, srcById)).join("")}</section>`
    )
    .join("");
  const srcRows = (a.sources || [])
    .map((s) => {
      const url = safeHttpUrl(s.final_url || s.requested_url || "");
      const openBtn = url
        ? `<button type="button" class="btn-ghost src-open" data-url="${escapeHtml(url)}">Open ↗</button>`
        : "";
      return `<li class="src-row">
        <span class="src-id num">${escapeHtml(s.id || "")}</span>
        <span class="src-domain">${escapeHtml(s.domain || domainOf(url))}</span>
        <span class="src-status src-status-${escapeHtml(String(s.status || ""))}">${escapeHtml(
          String(s.status || "")
        )} · ${escapeHtml(String(s.kind || ""))}</span>
        ${openBtn}
      </li>`;
    })
    .join("");
  const lims = (a.limitations || []).map((l) => `<li>${escapeHtml(l)}</li>`).join("");
  const inner = `
    <div class="card-head">
      <span class="card-title">Research</span>
      <span class="card-sub">confidence: ${escapeHtml(String(a.confidence || ""))}</span>
    </div>
    <p class="answer-summary">${escapeHtml(a.summary?.text || "")} ${citeRefs(
    a.summary?.citations,
    srcById
  )}</p>
    ${sections}
    ${lims ? `<div class="answer-limitations"><span class="card-note">Limitations</span><ul>${lims}</ul></div>` : ""}
    <details class="source-tray">
      <summary>Consulted sources (${(a.sources || []).length})</summary>
      <ul class="src-list">${srcRows}</ul>
    </details>`;
  return cardShell("research_answer", inner);
}

function renderResearchDigest(card) {
  const d = card.digest || {};
  const rows = (d.items || [])
    .map((it) => {
      const url = safeHttpUrl(it.url || "");
      const openBtn = url
        ? `<button type="button" class="btn-ghost src-open" data-url="${escapeHtml(url)}">Open ↗</button>`
        : "";
      return `<li class="hit-row">
        <div class="hit-main">
          <span class="hit-title">${escapeHtml(it.title || domainOf(url))}</span>
          <span class="src-status src-status-${escapeHtml(String(it.status || ""))}">${escapeHtml(
        String(it.status || "")
      )}</span>
        </div>
        ${it.snippet ? `<p class="hit-snippet">${escapeHtml(it.snippet)}</p>` : ""}
        ${openBtn}
      </li>`;
    })
    .join("");
  const lims = (d.limitations || []).map((l) => `<li>${escapeHtml(l)}</li>`).join("");
  const inner = `
    <div class="card-head"><span class="card-title">Source digest — no synthesis</span></div>
    <ul class="hit-list">${rows || '<li class="card-note">No sources.</li>'}</ul>
    ${lims ? `<div class="answer-limitations"><ul>${lims}</ul></div>` : ""}`;
  return cardShell("research_digest", inner);
}

// ── dispatch + interaction ──────────────────────────────────────────
export function renderCard(card) {
  if (!card || typeof card !== "object") return document.createComment("empty card");
  let el;
  switch (card.type) {
    case "model": el = renderModel(card); break;
    case "benchmark": el = renderBenchmark(card); break;
    case "search": el = renderSearch(card); break;
    case "page": el = renderPage(card); break;
    case "news": el = renderNews(card); break;
    case "deal": el = renderDeal(card); break;
    case "quote": el = renderQuote(card); break;
    case "filings": el = renderFilings(card); break;
    case "filing_doc": el = renderFilingDoc(card); break;
    case "assumptions": el = renderAssumptions(card); break;
    case "research_answer": el = renderResearchAnswer(card); break;
    case "research_digest": el = renderResearchDigest(card); break;
    case "error":
      el = cardShell("error", `<p class="card-note err">${escapeHtml(card.message || "Tool failed.")}</p>`);
      break;
    case "tool_contract":
      el = cardShell("error", `<p class="card-note err">${escapeHtml(card.message || "Invalid tool arguments.")}</p>`);
      break;
    default:
      el = cardShell("unknown", `<p class="card-note">${escapeHtml(card.type || "result")}</p>`);
  }
  wireCard(el);
  return el;
}

function wireCard(el) {
  el.addEventListener("click", async (e) => {
    // Assumptions build.
    const buildBtn = e.target.closest("[data-build-assumptions]");
    if (buildBtn) {
      e.stopPropagation();
      await buildFromAssumptions(el, buildBtn.dataset.buildAssumptions);
      return;
    }
    // Excel / folder.
    const excel = e.target.closest("[data-open-excel]");
    if (excel) {
      e.stopPropagation();
      openPath(excel.dataset.openExcel);
      return;
    }
    const folder = e.target.closest("[data-show-folder]");
    if (folder) {
      e.stopPropagation();
      openPath(parentDir(folder.dataset.showFolder));
      return;
    }
    // Analyst tools (Phase 6.5): EV / IFRS / tie-out.
    const analyst = e.target.closest("[data-analyst]");
    if (analyst) {
      e.stopPropagation();
      openAnalyst();
      return;
    }
    // External link/button (checked before reader row so the Open button wins).
    const ext = e.target.closest("[data-url]");
    if (ext) {
      e.stopPropagation();
      openExternal(ext.dataset.url);
      return;
    }
    // Reader row/button.
    const reader = e.target.closest("[data-reader-url]");
    if (reader) {
      openReader(reader.dataset.readerUrl, reader.dataset.readerTitle || "");
    }
  });
  // Keyboard activation for clickable rows.
  el.addEventListener("keydown", (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    const row = e.target.closest("[data-reader-url],[data-url]");
    if (row && (row.getAttribute("role") === "button")) {
      e.preventDefault();
      row.click();
    }
  });
}

async function buildFromAssumptions(cardEl, sessionId) {
  const status = cardEl.querySelector(".assumptions-status");
  const btn = cardEl.querySelector("[data-build-assumptions]");
  const overrides = collectOverrides(cardEl);
  if (btn) btn.disabled = true;
  if (status) {
    status.hidden = false;
    status.textContent = "Building…";
  }
  try {
    const summary = await call("finalize_model", {
      session_id: sessionId,
      options: { assumption_overrides: overrides },
    });
    const modelCard = {
      type: "model",
      ticker: summary.ticker,
      currency: summary.currency,
      xlsx_path: summary.xlsx_path,
      valuation: summary.valuation,
    };
    cardEl.replaceWith(renderCard(modelCard));
  } catch (err) {
    if (btn) btn.disabled = false;
    if (status) status.textContent = `Build failed: ${err && err.message ? err.message : err}`;
  }
}
