// cards.mjs — inline result cards for chat tool outputs. Each card is the only
// card treatment in the UI (they are interactive). renderCard(card) → element.

import {
  call,
  escapeHtml,
  openExternal,
  deepSourceUrl,
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
// Open a path via the OS; on failure surface a short hint on the card rather
// than a dead click. openPath resolves false when the path isn't a registered
// artifact or the opener errors.
async function openFileOrHint(cardEl, path) {
  const ok = await openPath(path);
  if (ok) return;
  const hint = cardEl && cardEl.querySelector(".open-fail-hint");
  if (hint) {
    hint.textContent = "Couldn't open it — the file may have moved or been cleaned up. Re-run the draft to regenerate it.";
    hint.hidden = false;
  }
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

// Data room review: per-question answers where every finding is a chip -
// file, page, verified badge - and the quote is one hover away. Click a
// chip to open the document itself.
function renderDataRoom(card) {
  const qs = Array.isArray(card.questions) ? card.questions : [];
  if (!qs.length) return null;
  const rootLeaf = String(card.root || "").replace(/\\/g, "/").split("/").filter(Boolean).pop() || "data room";
  const sections = qs
    .map((q) => {
      const findings = (Array.isArray(q.findings) ? q.findings : [])
        .map((f) => {
          const label = `${f.file || ""}${f.page ? ` · p.${f.page}` : ""}`;
          const badge = f.verified
            ? '<span class="room-verified" title="Quote verified verbatim against the document">✓</span>'
            : '<span class="room-unverified" title="Couldn\u2019t verify this quote verbatim - open the file to check">?</span>';
          return `<button type="button" class="room-finding" data-room-file="${escapeHtml(f.path || "")}" title="${escapeHtml(String(f.quote || ""))}">${escapeHtml(label)} ${badge}</button>`;
        })
        .join("");
      return `<div class="room-q">
        <p class="room-question">${escapeHtml(q.question || "")}</p>
        <p class="card-note">${escapeHtml(q.answer || "")}</p>
        ${findings ? `<div class="room-findings">${findings}</div>` : ""}
      </div>`;
    })
    .join("");
  const skipped = Array.isArray(card.skipped) && card.skipped.length
    ? `<details class="delegate-trail"><summary>Not read (${card.skipped.length})</summary><ol>${card.skipped
        .slice(0, 20)
        .map((s) => `<li>${escapeHtml(String(s))}</li>`)
        .join("")}</ol></details>`
    : "";
  const sub = `${card.file_count || 0} files · ${card.findings || 0} findings · ${card.verified || 0} verified`;
  return cardShell(
    "research data-room",
    `<div class="card-head">
       <span class="card-title">Data room · ${escapeHtml(rootLeaf)}</span>
       <span class="card-sub">${escapeHtml(sub)}</span>
     </div>
     ${sections}
     ${skipped}`,
  );
}

// A delegated deep dive: the child analyst's findings brief, with the task
// as the card title so parallel dives stay tellable-apart.
function renderDelegate(card) {
  const task = String(card.task || "Deep dive");
  const who = card.agent ? `Agent ${String(card.agent)}` : "Deep dive";
  const findings = String(card.findings || "");
  if (!findings) return null;
  const tools = Array.isArray(card.tools_used) && card.tools_used.length
    ? `<p class="card-sub">Checked with: ${escapeHtml(card.tools_used.join(", "))}</p>`
    : "";
  const paras = findings
    .split(/\n{2,}/)
    .slice(0, 6)
    .map((p) => `<p class="card-note">${escapeHtml(p.trim())}</p>`)
    .join("");
  const trail = Array.isArray(card.trail) ? card.trail.filter(Boolean) : [];
  const trailHtml = trail.length
    ? `<details class="delegate-trail"><summary>How this was worked (${trail.length} ${trail.length === 1 ? "check" : "checks"})</summary><ol>${trail
        .slice(0, 12)
        .map((t) => {
          const subject = t.subject ? ` · ${escapeHtml(String(t.subject))}` : "";
          const note = t.note ? ` — ${escapeHtml(String(t.note))}` : "";
          return `<li><span class="trail-tool">${escapeHtml(String(t.tool || ""))}</span>${subject}${note}</li>`;
        })
        .join("")}</ol></details>`
    : "";
  return cardShell(
    "research",
    `<div class="card-head">
       <span class="card-title">${escapeHtml(who)} · ${escapeHtml(task.length > 80 ? task.slice(0, 80) + "…" : task)}</span>
     </div>
     ${paras}
     ${trailHtml}
     ${tools}`,
  );
}

// A dispatched swarm: the whole army's briefs at a glance, one panel per
// subagent, so parallel work stays legible instead of scattered through the
// log. Mirrors the delegate card's treatment per panel (findings + trail).
function renderSwarm(card) {
  const agents = Array.isArray(card.agents) ? card.agents.filter(Boolean) : [];
  if (!agents.length) return null;
  const okCount = Number(
    card.ok_count != null ? card.ok_count : agents.filter((a) => a.ok).length,
  );
  const context = card.context ? String(card.context) : "";
  const contextChip = context
    ? ` · ${escapeHtml(context.length > 92 ? context.slice(0, 92) + "…" : context)}`
    : "";
  const head = `<div class="card-head">
       <span class="card-title">Swarm · ${agents.length} subagent${agents.length === 1 ? "" : "s"}</span>
       <span class="card-sub">${okCount}/${agents.length} returned a brief${contextChip}</span>
     </div>`;
  const panels = agents
    .map((a) => {
      const worker = a.agent ? `Agent ${String(a.agent)}` : "Deep dive";
      const name = a.name ? String(a.name) : worker;
      const workerMeta =
        name === worker ? "" : `<span class="swarm-worker">${escapeHtml(worker)}</span>`;
      const task = String(a.task || "");
      const taskHead = escapeHtml(task.length > 90 ? task.slice(0, 90) + "…" : task);
      if (!a.ok) {
        return `<div class="swarm-agent swarm-failed">
           <p class="swarm-agent-title">${escapeHtml(name)} ${workerMeta}</p>
           <p class="swarm-task">${taskHead}</p>
           <p class="card-note">Didn't finish — ${escapeHtml(String(a.error || "no brief"))}</p>
         </div>`;
      }
      const findings = String(a.findings || "");
      const paras = findings
        .split(/\n{2,}/)
        .slice(0, 4)
        .map((p) => `<p class="card-note">${escapeHtml(p.trim())}</p>`)
        .join("");
      const tools = Array.isArray(a.tools_used) && a.tools_used.length
        ? `<p class="card-sub">Checked with: ${escapeHtml(a.tools_used.join(", "))}</p>`
        : "";
      const trail = Array.isArray(a.trail) ? a.trail.filter(Boolean) : [];
      const trailHtml = trail.length
        ? `<details class="delegate-trail"><summary>How this was worked (${trail.length} ${trail.length === 1 ? "check" : "checks"})</summary><ol>${trail
            .slice(0, 12)
            .map((t) => {
              const subject = t.subject ? ` · ${escapeHtml(String(t.subject))}` : "";
              const note = t.note ? ` — ${escapeHtml(String(t.note))}` : "";
              return `<li><span class="trail-tool">${escapeHtml(String(t.tool || ""))}</span>${subject}${note}</li>`;
            })
            .join("")}</ol></details>`
        : "";
      return `<div class="swarm-agent">
         <p class="swarm-agent-title">${escapeHtml(name)} ${workerMeta}</p>
         <p class="swarm-task">${taskHead}</p>
         ${paras}
         ${trailHtml}
         ${tools}
       </div>`;
    })
    .join("");
  return cardShell("research swarm", `${head}<div class="swarm-grid">${panels}</div>`);
}

// Per-turn spend transparency: one quiet line. Tokens always; dollars
// only when the provider actually billed something measurable.
function renderTurnCost(card) {
  const p = Number(card.prompt_tokens || 0);
  const c = Number(card.completion_tokens || 0);
  if (p + c === 0) return null;
  const tokens = p + c;
  const tokensLabel =
    tokens >= 1000 ? `${(tokens / 1000).toFixed(1)}k tokens` : `${tokens} tokens`;
  const usd =
    typeof card.usd === "number" && card.usd > 0
      ? ` · about $${card.usd.toFixed(card.usd < 0.1 ? 3 : 2)}`
      : "";
  return cardShell(
    "turn-cost",
    `<p class="card-sub turn-cost-line">This turn: ${escapeHtml(tokensLabel)}${escapeHtml(usd)}</p>`,
  );
}

// The advisor's "second look": short reviewer notes, rendered as a quiet
// list — the point is honesty, not alarm.
function renderAdvisor(card) {
  const notes = Array.isArray(card.notes) ? card.notes.filter(Boolean) : [];
  if (!notes.length) return null;
  const items = notes
    .map((t) => `<li>${escapeHtml(String(t))}</li>`)
    .join("");
  return cardShell(
    "verify advisor",
    `<div class="card-head">
       <span class="card-title">Second look</span>
     </div>
     <ul class="advisor-notes">${items}</ul>`,
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
  // Auditability: filings[i] is the SEC filing the i-th column's figures
  // were reported in — every number in that column is one click from its
  // primary source.
  const filings = Array.isArray(card.filings) ? card.filings : [];
  const colLink = (i) => {
    const u = typeof filings[i] === "string" ? filings[i] : "";
    return /^https?:\/\//i.test(u) ? u : "";
  };
  const rows = (card.rows || [])
    .map((r) => {
      if (periods && Array.isArray(r.values)) {
        const cells = periods
          .map((p, i) => {
            const v = r.values[i] != null ? String(r.values[i]) : "—";
            const u = colLink(i);
            if (!u || v === "—") return `<td class="num">${escapeHtml(v)}</td>`;
            return `<td class="num"><span class="num-link" data-url="${escapeHtml(u)}" role="link" tabindex="0" title="Reported in the ${escapeHtml(p.label || "")} filing — opens it on SEC EDGAR">${escapeHtml(v)}</span></td>`;
          })
          .join("");
        const cls = r.kind === "derived" ? ' class="fin-derived"' : "";
        return `<tr${cls}><td>${escapeHtml(r.label || "")}</td>${cells}</tr>`;
      }
      return `<tr><td>${escapeHtml(r.label || "")}</td><td class="num">${escapeHtml(
        r.display != null ? String(r.display) : String(r.value ?? ""),
      )}</td></tr>`;
    })
    .join("");
  // Only bare years get the FY prefix — LTM / quarterly / "H1 FY25" labels
  // arrive pre-worded ("FYLTM" was nonsense).
  const fyRaw = card.fiscal_year ? String(card.fiscal_year) : "";
  const fy = fyRaw
    ? /^\d{4}$/.test(fyRaw)
      ? `FY${escapeHtml(fyRaw)}`
      : escapeHtml(fyRaw)
    : "";
  const sub = [
    fy,
    card.period_end ? `period ended ${escapeHtml(card.period_end)}` : "",
  ]
    .filter(Boolean)
    .join(" · ");
  // Source label from the actual venue — ESEF cards are not "SEC EDGAR".
  const srcLabel = /sec\.gov/.test(card.source || "")
    ? "SEC EDGAR"
    : /filings\.xbrl\.org/.test(card.source || "")
      ? "filings.xbrl.org (ESEF)"
      : "Source";
  const src = card.source
    ? `<div class="card-sources"><a href="#" class="md-link" data-url="${escapeHtml(card.source)}">${srcLabel}</a></div>`
    : "";
  // Basis toggle: the four bases an analyst flips between, re-fetched in
  // place. Half-year is how most EU/UK/JP companies report.
  const basisNow = String(card.basis || "annual").toLowerCase();
  const basisChips = card.ticker
    ? `<div class="basis-toggle" role="group" aria-label="Reporting basis">${[
        ["annual", "Annual"],
        ["quarterly", "Quarterly"],
        ["ltm", "LTM"],
        ["semi", "Half-year"],
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
            .map((p, i) => {
              const u = colLink(i);
              const label = escapeHtml(p.label || "");
              if (!u) return `<th scope="col" class="num">${label}</th>`;
              return `<th scope="col" class="num"><span class="num-link fy-link" data-url="${escapeHtml(u)}" role="link" tabindex="0" title="Open this fiscal year's filing on SEC EDGAR">${label}</span></th>`;
            })
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
  // In-chat preview: the drafted text itself, collapsed, so the deliverable
  // is readable without leaving the conversation (the user asked to see the
  // output in chat, not just a file path).
  const preview = String(card.preview || "").trim();
  const previewHtml = preview
    ? `<details class="memo-preview" open><summary>Preview</summary><pre class="memo-preview-body">${escapeHtml(preview)}</pre></details>`
    : "";
  const inner = `
    <div class="card-head">
      <span class="card-title">${escapeHtml(card.company || "")}</span>
      <span class="card-sub">${escapeHtml(kind)} · ${escapeHtml(String(card.sections || 0))} sections · ${escapeHtml(String(card.sources || 0))} sources</span>
    </div>
    ${note}
    ${previewHtml}
    <div class="card-actions">
      <button type="button" class="btn-primary" data-open-excel="${escapeHtml(card.memo_path || "")}">Open memo</button>
      ${card.pptx_path ? `<button type="button" class="btn-ghost" data-open-excel="${escapeHtml(card.pptx_path)}">Open deck</button>` : ""}
      <button type="button" class="btn-ghost" data-show-folder="${escapeHtml(card.memo_path || "")}">Show in folder</button>
      <span class="open-fail-hint" hidden></span>
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
      // Text-fragment deep link: the browser opens the source scrolled to
      // and highlighting this citation's quote (plain URL when no quote).
      const deep = url ? deepSourceUrl(url, c.quote) || url : "";
      const attrs = deep ? ` data-url="${escapeHtml(deep)}"` : "";
      const ord = order.indexOf(c.source_id);
      const chip = citeChipLabel(src, c.source_id, ord >= 0 ? ord : 0);
      const publisher =
        sourcePublisherLabel(src, url) || c.source_id || "source";
      return `<button type="button" class="cite-ref"${attrs} title="${escapeHtml(
        c.quote ? `Opens the source at: \u201c${c.quote}\u201d` : publisher,
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
      // Anchor on the source's own verbatim snippet/excerpt so the click
      // lands at the cited passage, not just the page top.
      const deep = url ? deepSourceUrl(url, src.excerpt || src.snippet) || url : "";
      const clickable = deep
        ? ` data-url="${escapeHtml(deep)}" role="button" tabindex="0" title="Opens the source at the cited passage"`
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
    case "advisor":
      el = renderAdvisor(card);
      break;
    case "delegate":
      el = renderDelegate(card);
      break;
    case "swarm":
      el = renderSwarm(card);
      break;
    case "data_room":
      el = renderDataRoom(card);
      break;
    case "turn_cost":
      el = renderTurnCost(card);
      break;
    case "self_check":
      el = cardShell(
        "verify",
        `<p class="card-note">${escapeHtml(card.message || "Caught figures with no tool behind them — checking properly.")}</p>`,
      );
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
  // Renderers may decline (advisor with no notes, delegate with no
  // findings) — same convention as the empty-card guard above.
  if (!el) return document.createComment("empty card");
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
    // Excel / folder. openPath resolves false when the path isn't a
    // registered artifact or the OS can't open it — surface that instead of
    // failing silently (the old behavior read as a dead button).
    const excel = e.target.closest("[data-open-excel]");
    if (excel) {
      e.stopPropagation();
      openFileOrHint(el, excel.dataset.openExcel);
      return;
    }
    const folder = e.target.closest("[data-show-folder]");
    if (folder) {
      e.stopPropagation();
      openFileOrHint(el, parentDir(folder.dataset.showFolder));
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
        .catch((err) => {
          chips.forEach((c) => (c.disabled = false));
          // Say why, quietly — a US filer has no half-years, and silence
          // reads like a broken button.
          basisBtn.title = (err && err.message) || "That view isn't available for this company";
          basisBtn.classList.add("basis-unavailable");
          setTimeout(() => basisBtn.classList.remove("basis-unavailable"), 1600);
        });
      return;
    }
    // Data room finding chip: open the cited file locally.
    const roomFile = e.target.closest("[data-room-file]");
    if (roomFile) {
      e.stopPropagation();
      openPath(roomFile.dataset.roomFile);
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
  // Keyboard activation for clickable rows and inline links. Enter works
  // for both; Space is button-only (links must not swallow page scroll).
  el.addEventListener("keydown", (e) => {
    if (e.key !== "Enter" && e.key !== " ") return;
    const row = e.target.closest("[data-reader-url],[data-url]");
    if (!row) return;
    const role = row.getAttribute("role");
    if (role === "button" || (role === "link" && e.key === "Enter")) {
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
