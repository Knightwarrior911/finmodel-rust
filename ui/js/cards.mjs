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
import {
  confidenceLabel,
  verifyStatusLabel,
  pageStatusLabel,
  sourceStatusLabel,
  dealSufficiencyLabel,
  factKeyLabel,
  formatFactValue,
  citeChipLabel,
  sourcePublisherLabel,
  sourceCardTitle,
  sourceAvatarLetter,
  sourceRowMeta,
  softErrorMessage,
  filingFormLabel,
  filingItemLabel,
  memoKindLabel,
} from "./labels.mjs";

function parentDir(p) {
  return String(p || "").replace(/[\\/][^\\/]+$/, "");
}

function cardShell(kind, inner) {
  const el = document.createElement("div");
  el.className = `card card-${kind}`;
  el.innerHTML = inner;
  return el;
}

// ── verification ────────────────────────────────────────────────────
// The analyst's verify step (Task 4.2): shows how many material figures were
// checked against their primary source and the rolled-up run badge.
function renderVerification(card) {
  const status = card.status || "partial_unverified";
  const label = verifyStatusLabel(status);
  const verified = Number(card.verified || 0);
  const total = Number(card.total || 0);
  const src = card.source ? ` against ${escapeHtml(card.source)}` : "";
  const note =
    total > 0
      ? `${verified} of ${total} key figures checked${card.source ? " against " + card.source : ""}.`
      : "No figures to check yet.";
  return cardShell(
    `verify status-${escapeHtml(status)}`,
    `<div class="card-head">
       <span class="card-title">${escapeHtml(label)}</span>
       <span class="verify-badge status-${escapeHtml(status)}">${escapeHtml(String(verified))}/${escapeHtml(String(total))}</span>
     </div>
     <p class="card-note">${escapeHtml(note)}</p>`,
  );
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
      ? `<p class="card-note">Compared with ${comps.count} peer${comps.count === 1 ? "" : "s"}${
          comps.excluded && comps.excluded.length
            ? ` (${comps.excluded.length} set aside)`
            : ""
        }</p>`
      : "";
  const caseTag =
    card.case && card.case !== "base"
      ? `<span class="card-tag">${escapeHtml(card.case === "upside" ? "Upside case" : "Downside case")}</span>`
      : "";
  const inner = `
    <div class="card-head">
      <span class="card-title num">${escapeHtml(card.ticker || "")}</span>
      <span class="card-sub">${escapeHtml(card.currency || "")}${card.currency ? " · " : ""}Workbook ready</span>
      ${caseTag}
    </div>
    ${valuationStrip(v)}
    ${compsNote}
    <div class="card-actions">
      <button type="button" class="btn-primary" data-open-excel="${escapeHtml(card.xlsx_path || "")}">Open in Excel</button>
      ${card.pptx_path ? `<button type="button" class="btn-ghost" data-open-excel="${escapeHtml(card.pptx_path)}">Open deck</button>` : ""}
      <button type="button" class="btn-ghost" data-show-folder="${escapeHtml(card.xlsx_path || "")}">Show in folder</button>
      <button type="button" class="btn-ghost" data-analyst="1">More model tools</button>
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
          const val =
            h.key === "ticker"
              ? escapeHtml(raw || "")
              : escapeHtml((BENCH_FMT[h.key] || String)(raw));
          const numCls = h.key === "ticker" ? "" : ' class="num"';
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
        .map((h) =>
          h.key === "ticker"
            ? r[h.key] || ""
            : (BENCH_FMT[h.key] || String)(r[h.key]),
        )
        .join("\t"),
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
      (
        h,
      ) => `<li class="hit-row" data-reader-url="${escapeHtml(h.url)}" data-reader-title="${escapeHtml(
        h.title || "",
      )}" tabindex="0" role="button">
        <div class="hit-main">
          <span class="hit-title">${escapeHtml(h.title || domainOf(h.url))}</span>
          <span class="hit-domain num">${escapeHtml(domainOf(h.url))}</span>
        </div>
        ${h.snippet ? `<p class="hit-snippet">${escapeHtml(h.snippet)}</p>` : ""}
        <button type="button" class="btn-ghost hit-open" data-url="${escapeHtml(h.url)}">Open ↗</button>
      </li>`,
    )
    .join("");
  const title = card.query
    ? `Web results for ${escapeHtml(card.query)}`
    : "Web results";
  const inner = `
    <div class="card-head"><span class="card-title">${title}</span></div>
    <ul class="hit-list">${rows || '<li class="card-note">No matching results.</li>'}</ul>`;
  return cardShell("search", inner);
}

// ── page ────────────────────────────────────────────────────────────
function renderPage(card) {
  const statusText = pageStatusLabel(card.status);
  const domain = domainOf(card.url);
  const sub = [domain, statusText].filter(Boolean).join(" · ");
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(card.title || domain)}</span>
      <span class="card-sub">${escapeHtml(sub)}</span>
    </div>
    <div class="card-actions">
      <button type="button" class="btn-primary" data-reader-url="${escapeHtml(card.url)}" data-reader-title="${escapeHtml(card.title || "")}">Open in reader</button>
      <button type="button" class="btn-ghost" data-url="${escapeHtml(card.url)}">Open in browser ↗</button>
    </div>`;
  return cardShell("page", inner);
}

// ── news ────────────────────────────────────────────────────────────
function renderNews(card) {
  const items = card.items || [];
  const rows = items
    .map(
      (
        n,
      ) => `<li class="news-row" data-url="${escapeHtml(n.url)}" role="button" tabindex="0">
        <span class="news-title">${escapeHtml(n.title)}</span>
        <span class="news-src num">${escapeHtml(n.source || "")}${n.published ? " · " + escapeHtml(n.published) : ""}</span>
      </li>`,
    )
    .join("");
  const title = card.query
    ? `Headlines for ${escapeHtml(card.query)}`
    : "Headlines";
  const inner = `
    <div class="card-head"><span class="card-title">${title}</span></div>
    <ul class="news-list">${rows || '<li class="card-note">No recent headlines.</li>'}</ul>`;
  return cardShell("news", inner);
}

// ── deal ────────────────────────────────────────────────────────────
function renderDeal(card) {
  const summary = card.summary || {};
  const facts = Object.entries(summary)
    .filter(([, v]) => v != null && v !== "")
    .map(([k, v]) => {
      const pretty = formatFactValue(v);
      if (!pretty) return "";
      return `<div class="fact"><span class="fact-k">${escapeHtml(factKeyLabel(k))}</span><span class="fact-v">${escapeHtml(pretty)}</span></div>`;
    })
    .filter(Boolean)
    .join("");
  const sources = (card.sources_read || [])
    .map((u, i) => {
      const domain = domainOf(u) || "Source";
      const letter = sourceAvatarLetter(domain);
      return `<li class="src-card" data-url="${escapeHtml(u)}" role="button" tabindex="0">
        <span class="src-card-num num" aria-hidden="true">${i + 1}</span>
        <span class="src-card-avatar" aria-hidden="true">${escapeHtml(letter)}</span>
        <span class="src-card-body">
          <span class="src-card-title">${escapeHtml(domain)}</span>
          <span class="src-card-meta">${escapeHtml(domain)}</span>
        </span>
      </li>`;
    })
    .join("");
  const head =
    [card.acquirer, card.target].filter(Boolean).join(" / ") || "Deal research";
  const nSrc = (card.sources_read || []).length;
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(head)}</span>
      <span class="card-sub">${escapeHtml(dealSufficiencyLabel(card.sufficient))}</span>
    </div>
    <div class="fact-grid">${facts || '<p class="card-note">No deal facts pulled yet.</p>'}</div>
    ${sources ? `<div class="source-strip"><div class="source-strip-head">Sources${nSrc ? ` · ${nSrc}` : ""}</div><ul class="src-cards">${sources}</ul></div>` : ""}`;
  return cardShell("deal", inner);
}

// ── quote ───────────────────────────────────────────────────────────
function renderQuote(card) {
  const range =
    card.week52_low != null && card.week52_high != null
      ? `<span class="quote-range num">52-week ${escapeHtml(fmtPrice(card.week52_low))} – ${escapeHtml(fmtPrice(card.week52_high))}</span>`
      : "";
  const inner = `
    <div class="card-head">
      <span class="card-title num">${escapeHtml(card.ticker || "")}</span>
      <span class="card-sub">Last price</span>
    </div>
    <div class="quote-line">
      <span class="quote-price num">${escapeHtml(fmtPrice(card.price))}</span>
      <span class="quote-ccy">${escapeHtml(card.currency || "")}</span>
      ${range}
    </div>`;
  return cardShell("quote", inner);
}

// ── filings ─────────────────────────────────────────────────────────
function renderFilings(card) {
  const rows = (card.rows || [])
    .map(
      (
        f,
      ) => `<tr class="filing-row" data-url="${escapeHtml(f.url)}" role="button" tabindex="0">
        <td class="num">${escapeHtml(f.form_type || "")}</td>
        <td class="num">${escapeHtml(f.filing_date || "")}</td>
        <td class="num">${escapeHtml(f.fiscal_period_end || "")}</td>
      </tr>`,
    )
    .join("");
  const inner = `
    <div class="card-head"><span class="card-title num">${escapeHtml(card.ticker || "")}</span><span class="card-sub">filings</span></div>
    <div class="card-table-wrap"><table class="card-table"><thead><tr><th scope="col">Form</th><th scope="col">Filed</th><th scope="col">Period end</th></tr></thead><tbody>${
      rows || '<tr><td colspan="3" class="card-note">No filings found yet.</td></tr>'
    }</tbody></table></div>`;
  return cardShell("filings", inner);
}

// ── financials (exact reported figures from SEC EDGAR XBRL) ──────────
// Multi-year spread (card.periods + rows[].values) with a legacy fallback
// for single-year cards persisted in older conversations.
function renderFinancials(card) {
  const periods = Array.isArray(card.periods) ? card.periods : null;
  const rows = (card.rows || [])
    .map((r) => {
      if (periods && Array.isArray(r.values)) {
        const cells = periods
          .map(
            (_, i) =>
              `<td class="num">${escapeHtml(r.values[i] != null ? String(r.values[i]) : "—")}</td>`,
          )
          .join("");
        const cls = r.kind === "derived" ? ' class="fin-derived"' : "";
        return `<tr${cls}><td>${escapeHtml(r.label || "")}</td>${cells}</tr>`;
      }
      return `<tr><td>${escapeHtml(r.label || "")}</td><td class="num">${escapeHtml(
        r.display != null ? String(r.display) : String(r.value ?? ""),
      )}</td></tr>`;
    })
    .join("");
  const fy = card.fiscal_year
    ? `FY${escapeHtml(String(card.fiscal_year))}`
    : "";
  const sub = [
    fy,
    card.period_end ? `period ended ${escapeHtml(card.period_end)}` : "",
  ]
    .filter(Boolean)
    .join(" · ");
  const src = card.source
    ? `<div class="card-sources"><a href="#" class="md-link" data-url="${escapeHtml(card.source)}">SEC EDGAR</a></div>`
    : "";
  // Basis toggle: Annual / Quarterly / LTM re-fetch this ticker's card in
  // place — the three real bases an analyst flips between.
  const basisNow = String(card.basis || "annual").toLowerCase();
  const basisChips = card.ticker
    ? `<div class="basis-toggle" role="group" aria-label="Reporting basis">${[
        ["annual", "Annual"],
        ["quarterly", "Quarterly"],
        ["ltm", "LTM"],
      ]
        .map(
          ([b, label]) =>
            `<button type="button" class="basis-chip${b === basisNow ? " active" : ""}" data-basis="${b}" data-basis-ticker="${escapeHtml(card.ticker)}" aria-pressed="${b === basisNow}">${label}</button>`,
        )
        .join("")}</div>`
    : "";
  // Business segments (annual only; from the filing XBRL instance).
  const segRows = Array.isArray(card.segments)
    ? card.segments
        .map(
          (g) =>
            `<tr${g.eliminations ? ' class="fin-derived"' : ""}><td>${escapeHtml(g.segment)}${g.eliminations ? " (eliminations)" : ""}</td><td class="num">${escapeHtml(Number(g.value / 1e6).toLocaleString("en-US", { maximumFractionDigits: 0 }))}</td></tr>`,
        )
        .join("")
    : "";
  const segments = segRows
    ? `<div class="fin-segments"><div class="dock-section-head">Segment revenue · ${escapeHtml(card.currency || "USD")} millions</div><table class="card-table"><tbody>${segRows}</tbody></table></div>`
    : "";
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(card.entity || card.ticker || "Financials")}</span>
      ${sub ? `<span class="card-sub">${sub}</span>` : ""}
    </div>
    ${basisChips}
    <div class="card-table-wrap"><table class="card-table"><thead><tr><th scope="col">Line item</th>${
      periods
        ? periods
            .map((p) => `<th scope="col" class="num">${escapeHtml(p.label || "")}</th>`)
            .join("")
        : `<th scope="col">${escapeHtml(card.currency || "Value")}</th>`
    }</tr></thead><tbody>${
      rows || '<tr><td colspan="2" class="card-note">No figures to show yet.</td></tr>'
    }</tbody></table></div>
    ${segments}
    ${src}`;
  return cardShell("financials", inner);
}

// ── filing_doc (10-K/10-Q reader) ───────────────────────────────────
function renderFilingDoc(card) {
  const form = card.form || "";
  const formName = filingFormLabel(form);
  const sub = [
    escapeHtml(form),
    formName && formName !== form ? escapeHtml(formName) : null,
    card.filing_date ? escapeHtml(card.filing_date) : null,
  ]
    .filter(Boolean)
    .join(" · ");
  // A section read names the section it read; a whole-document open lists
  // what's inside. Never both — the chip wall next to a section line was
  // noise, and "Excerpt ready · N characters" told the reader nothing.
  const readLine = card.item
    ? `<p class="filing-read-line">Read ${escapeHtml(filingItemLabel(form, card.item))}</p>`
    : "";
  const chips = card.item
    ? ""
    : (card.items || [])
        .map(
          (id) =>
            `<span class="filing-item-chip">${escapeHtml(filingItemLabel(form, id))}</span>`,
        )
        .join("");
  const preview = card.preview
    ? `<blockquote class="filing-preview">${escapeHtml(card.preview)}</blockquote>`
    : "";
  const inner = `
    <div class="card-head">
      <span class="card-title num">${escapeHtml(card.ticker || "")}</span>
      <span class="card-sub">${sub}</span>
    </div>
    ${readLine}
    ${preview}
    ${chips ? `<div class="filing-items">${chips}</div>` : ""}
    <div class="card-actions">
      <button type="button" class="btn-ghost" data-url="${escapeHtml(card.url || "")}">Open on SEC.gov ↗</button>
    </div>`;
  return cardShell("filing_doc", inner);
}

// ── memo (drafted deliverable) ──────────────────────────────────────
function renderMemo(card) {
  const kind = memoKindLabel(card.kind);
  const fell = Number(card.fallback_sections || 0);
  const note =
    fell > 0
      ? `<p class="card-note">${fell} section${fell === 1 ? "" : "s"} composed directly from the evidence (drafting model text did not pass validation).</p>`
      : "";
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(card.company || "")}</span>
      <span class="card-sub">${escapeHtml(kind)} · ${escapeHtml(String(card.sections || 0))} sections · ${escapeHtml(String(card.sources || 0))} sources</span>
    </div>
    ${note}
    <div class="card-actions">
      <button type="button" class="btn-primary" data-open-excel="${escapeHtml(card.memo_path || "")}">Open memo</button>
      <button type="button" class="btn-ghost" data-show-folder="${escapeHtml(card.memo_path || "")}">Show in folder</button>
    </div>`;
  return cardShell("memo", inner);
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
      inp.classList.toggle(
        "edited",
        inp.value.trim() !== (inp.dataset.orig || ""),
      );
    });
  });
  return el;
}

function collectOverrides(cardEl) {
  const overrides = [];
  cardEl.querySelectorAll("tbody tr").forEach((tr) => {
    const values = Array.from(tr.querySelectorAll("input")).map((inp) => {
      if (!inp.classList.contains("edited") || inp.value.trim() === "")
        return null;
      const n = Number(inp.value.trim());
      return isFinite(n) ? n : null;
    });
    if (values.some((v) => v != null))
      overrides.push({ key: tr.dataset.key, values });
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

function citeRefs(citations, srcById, idOrder) {
  const order = Array.isArray(idOrder) ? idOrder : [];
  return (citations || [])
    .map((c) => {
      const src = srcById[c.source_id] || {};
      const url = safeHttpUrl(src.final_url || src.requested_url || "");
      const attrs = url ? ` data-url="${escapeHtml(url)}"` : "";
      const ord = order.indexOf(c.source_id);
      const chip = citeChipLabel(src, c.source_id, ord >= 0 ? ord : 0);
      const publisher =
        sourcePublisherLabel(src, url) || c.source_id || "source";
      return `<button type="button" class="cite-ref"${attrs} title="${escapeHtml(
        c.quote || publisher,
      )}" aria-label="Source ${escapeHtml(chip)}, ${escapeHtml(
        publisher,
      )}: ${escapeHtml(c.quote || "")}">${escapeHtml(chip)}</button>`;
    })
    .join("");
}

function citedPara(p, srcById, idOrder) {
  return `<p class="cited-para">${escapeHtml(p.text || "")} ${citeRefs(
    p.citations,
    srcById,
    idOrder,
  )}</p>`;
}

function renderResearchAnswer(card) {
  const a = card.answer || {};
  const srcById = {};
  const idOrder = [];
  for (const src of a.sources || []) {
    srcById[src.id] = src;
    idOrder.push(src.id);
  }
  const conf = confidenceLabel(a.confidence);
  const sections = (a.sections || [])
    .map(
      (sec) =>
        `<section class="answer-section"><h4 class="answer-heading">${escapeHtml(
          sec.heading || "",
        )}</h4>${(sec.paragraphs || [])
          .map((p) => citedPara(p, srcById, idOrder))
          .join("")}</section>`,
    )
    .join("");
  const srcCards = (a.sources || [])
    .map((src, i) => {
      const url = safeHttpUrl(src.final_url || src.requested_url || "");
      const domain = sourcePublisherLabel(src, url) || "Source";
      const title = sourceCardTitle(src, url);
      const letter = sourceAvatarLetter(domain);
      const meta = sourceRowMeta(src.status, src.kind);
      const clickable = url
        ? ` data-url="${escapeHtml(url)}" role="button" tabindex="0"`
        : "";
      return `<li class="src-card"${clickable}>
        <span class="src-card-num num" aria-hidden="true">${escapeHtml(
          citeChipLabel(src, src.id, i),
        )}</span>
        <span class="src-card-avatar" aria-hidden="true">${escapeHtml(letter)}</span>
        <span class="src-card-body">
          <span class="src-card-title">${escapeHtml(title)}</span>
          <span class="src-card-meta">${escapeHtml(domain)}${
            meta ? ` · ${escapeHtml(meta)}` : ""
          }</span>
        </span>
      </li>`;
    })
    .join("");
  const lims = (a.limitations || [])
    .map((l) => `<li>${escapeHtml(l)}</li>`)
    .join("");
  const n = (a.sources || []).length;
  const inner = `
    <div class="card-head">
      <span class="card-title">Research notes</span>
      ${conf ? `<span class="card-sub">${escapeHtml(conf)}</span>` : ""}
    </div>
    <p class="answer-summary">${escapeHtml(a.summary?.text || "")} ${citeRefs(
      a.summary?.citations,
      srcById,
      idOrder,
    )}</p>
    ${sections}
    ${lims ? `<div class="answer-limitations"><span class="card-note">Worth keeping in mind</span><ul>${lims}</ul></div>` : ""}
    ${
      n
        ? `<div class="source-strip">
      <div class="source-strip-head">Sources · ${n}</div>
      <ul class="src-cards">${srcCards}</ul>
    </div>`
        : ""
    }`;
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
      const st = sourceStatusLabel(it.status);
      return `<li class="hit-row">
        <div class="hit-main">
          <span class="hit-title">${escapeHtml(it.title || domainOf(url))}</span>
          ${st ? `<span class="src-status src-status-${escapeHtml(String(it.status || ""))}">${escapeHtml(st)}</span>` : ""}
        </div>
        ${it.snippet ? `<p class="hit-snippet">${escapeHtml(it.snippet)}</p>` : ""}
        ${openBtn}
      </li>`;
    })
    .join("");
  const lims = (d.limitations || [])
    .map((l) => `<li>${escapeHtml(l)}</li>`)
    .join("");
  const inner = `
    <div class="card-head"><span class="card-title">Sources I found</span><span class="card-sub">Collected before I could finish summarizing</span></div>
    <ul class="hit-list">${rows || '<li class="card-note">No sources yet.</li>'}</ul>
    ${lims ? `<div class="answer-limitations"><ul>${lims}</ul></div>` : ""}`;
  return cardShell("research_digest", inner);
}

// ── dispatch + interaction ──────────────────────────────────────────
export function renderCard(card) {
  if (!card || typeof card !== "object")
    return document.createComment("empty card");
  let el;
  switch (card.type) {
    case "model":
      el = renderModel(card);
      break;
    case "benchmark":
      el = renderBenchmark(card);
      break;
    case "search":
      el = renderSearch(card);
      break;
    case "page":
      el = renderPage(card);
      break;
    case "news":
      el = renderNews(card);
      break;
    case "deal":
      el = renderDeal(card);
      break;
    case "quote":
      el = renderQuote(card);
      break;
    case "filings":
      el = renderFilings(card);
      break;
    case "financials":
      el = renderFinancials(card);
      break;
    case "memo":
      el = renderMemo(card);
      break;
    case "filing_doc":
      el = renderFilingDoc(card);
      break;
    case "assumptions":
      el = renderAssumptions(card);
      break;
    case "research_answer":
      el = renderResearchAnswer(card);
      break;
    case "verification":
      el = renderVerification(card);
      break;
    case "research_digest":
      el = renderResearchDigest(card);
      break;
    case "error":
      el = cardShell(
        "error",
        `<p class="card-note err">${escapeHtml(card.message || softErrorMessage())}</p>`,
      );
      break;
    case "tool_contract":
      el = cardShell(
        "error",
        `<p class="card-note err">${escapeHtml(card.message || "I need a bit more detail to run that.")}</p>`,
      );
      break;
    default:
      el = cardShell(
        "unknown",
        `<p class="card-note">Result ready.</p>`,
      );
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
    // Financials basis toggle: re-fetch the card on the chosen basis and
    // swap it in place (no agent round, no scroll jump).
    const basisBtn = e.target.closest("[data-basis]");
    if (basisBtn) {
      e.stopPropagation();
      const shell = basisBtn.closest(".card");
      if (!shell || basisBtn.classList.contains("active")) return;
      const chips = shell.querySelectorAll(".basis-chip");
      chips.forEach((c) => (c.disabled = true));
      call("financials_card", {
        ticker: basisBtn.dataset.basisTicker,
        basis: basisBtn.dataset.basis,
      })
        .then((card) => {
          const next = renderCard(typeof card === "string" ? JSON.parse(card) : card);
          shell.replaceWith(next);
        })
        .catch(() => {
          chips.forEach((c) => (c.disabled = false));
        });
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
    if (row && row.getAttribute("role") === "button") {
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
    if (status)
      status.textContent = `Build failed: ${err && err.message ? err.message : err}`;
  }
}
