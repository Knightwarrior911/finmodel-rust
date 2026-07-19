// evidence.mjs — the Evidence dock's conversation-level ledger (Task 2.3).
//
// Every result card that flows through the chat (live `result_part_added` or
// history replay) also feeds this module, which maintains three views a
// working analyst keeps open beside the conversation:
//   Sources   — every source actually read or cited, deduped, in first-seen
//               order, numbered like the inline cites; click → Reader.
//   Valuation — the latest model valuation per ticker plus the last
//               verification verdict: the deal state at a glance.
//   Artifacts — workbooks and decks, newest first; click → open the file.
//
// State is pure and renderers take explicit elements/handlers so the whole
// module is testable in jsdom without Tauri.

import { $, escapeHtml, domainOf, openPath } from "./core.mjs";
import { openReader } from "./reader.mjs";
import { openDock } from "./workbench.mjs";
import {
  sourcePublisherLabel,
  sourceCardTitle,
  sourceAvatarLetter,
  sourceRowMeta,
  verifyStatusLabel,
} from "./labels.mjs";

// ── state ────────────────────────────────────────────────────────────

export function createEvidenceState() {
  return {
    // url -> index into list (dedup); list keeps first-seen order.
    sources: [],
    sourceIndex: new Map(),
    // newest first.
    artifacts: [],
    // ticker -> latest valuation snapshot; insertion order preserved.
    valuations: new Map(),
    verification: null,
  };
}

function addSource(state, src) {
  const url = (src.url || "").trim();
  if (!url) return false;
  const seen = state.sourceIndex.get(url);
  if (seen != null) {
    // A later read can upgrade what we know (status/title), never renumber.
    const cur = state.sources[seen];
    if (src.status) cur.status = src.status;
    if (src.title && !cur.title) cur.title = src.title;
    return false;
  }
  state.sourceIndex.set(url, state.sources.length);
  state.sources.push({
    url,
    title: src.title || "",
    domain: src.domain || domainOf(url),
    status: src.status || "",
    kind: src.kind || "",
  });
  return true;
}

function addArtifact(state, art) {
  if (!art.path) return false;
  // Same path re-built (new case run) floats to the top instead of duplicating.
  state.artifacts = state.artifacts.filter((a) => a.path !== art.path);
  state.artifacts.unshift(art);
  return true;
}

/// Feed one result card. Returns true when any panel changed.
export function evidenceAddCard(state, card) {
  if (!card || typeof card !== "object") return false;
  let changed = false;
  const answer = card.answer || {};

  // Sources: research answers, digests, deal reads, read pages, filings.
  for (const src of answer.sources || card.sources || []) {
    changed =
      addSource(state, {
        url: src.final_url || src.requested_url || src.url || "",
        title: sourceCardTitle(src, src.final_url || src.requested_url || ""),
        domain: sourcePublisherLabel(src, src.final_url || src.requested_url || ""),
        status: src.status,
        kind: src.kind,
      }) || changed;
  }
  if (card.type === "research_digest") {
    for (const it of (card.digest && card.digest.items) || []) {
      changed =
        addSource(state, {
          url: it.url,
          title: it.title,
          status: it.status,
        }) || changed;
    }
  }
  for (const u of card.sources_read || []) {
    changed = addSource(state, { url: u }) || changed;
  }
  if (card.type === "page" && card.url) {
    changed =
      addSource(state, { url: card.url, title: card.title, status: card.status }) ||
      changed;
  }
  if (card.type === "filing_doc" && card.url) {
    const bits = [card.ticker, card.form, card.filing_date]
      .filter(Boolean)
      .join(" · ");
    changed =
      addSource(state, { url: card.url, title: bits, kind: "filing", status: "read" }) ||
      changed;
  }

  // Artifacts: any card carrying a workbook/deck path.
  if (card.xlsx_path) {
    changed =
      addArtifact(state, {
        path: card.xlsx_path,
        label: card.ticker ? `${card.ticker} model` : "Workbook",
        kind: "workbook",
      }) || changed;
  }
  if (card.memo_path) {
    changed =
      addArtifact(state, {
        path: card.memo_path,
        label: card.company ? `${card.company} memo` : "Memo",
        kind: "memo",
      }) || changed;
  }
  if (card.pptx_path) {
    changed =
      addArtifact(state, {
        path: card.pptx_path,
        label: card.ticker
          ? `${card.ticker} deck`
          : card.company
            ? `${card.company} deck`
            : "Deck",
        kind: "deck",
      }) || changed;
  }

  // Valuation: latest model snapshot per ticker + last verification verdict.
  if (card.type === "model" && card.ticker) {
    state.valuations.set(card.ticker, {
      ticker: card.ticker,
      currency: card.currency || "",
      ...(card.valuation || {}),
    });
    changed = true;
  }
  if (card.type === "verification" && card.status) {
    state.verification = {
      status: card.status,
      verified: card.verified,
      total: card.total,
    };
    changed = true;
  }
  return changed;
}

// ── rendering ────────────────────────────────────────────────────────

const fmtNum = (v) =>
  v == null || Number.isNaN(Number(v))
    ? ""
    : Number(v).toLocaleString("en-US", { maximumFractionDigits: 2 });
const fmtPct = (v) => (v == null ? "" : `${fmtNum(v)}%`);

export function renderEvidenceSources(el, state, { onOpen } = {}) {
  if (!el) return;
  if (state.sources.length === 0) {
    el.innerHTML = `<p class="dock-empty">Sources I cite will collect here as we go.</p>`;
    return;
  }
  const rows = state.sources
    .map((s, i) => {
      const title = s.title || s.domain || s.url;
      const meta = [s.domain, sourceRowMeta(s.status, s.kind)]
        .filter(Boolean)
        .join(" · ");
      return `<li class="src-card dock-src" data-src-url="${escapeHtml(s.url)}" data-src-title="${escapeHtml(title)}" role="button" tabindex="0">
        <span class="src-card-num num" aria-hidden="true">${i + 1}</span>
        <span class="src-card-avatar" aria-hidden="true">${escapeHtml(sourceAvatarLetter(s.domain || title))}</span>
        <span class="src-card-body">
          <span class="src-card-title">${escapeHtml(title)}</span>
          <span class="src-card-meta">${escapeHtml(meta)}</span>
        </span>
      </li>`;
    })
    .join("");
  el.innerHTML = `<div class="dock-section-head">Sources · ${state.sources.length}</div>
    <ul class="src-cards dock-src-list">${rows}</ul>`;
  if (onOpen) {
    el.querySelectorAll("[data-src-url]").forEach((row) => {
      row.addEventListener("click", () =>
        onOpen(row.dataset.srcUrl, row.dataset.srcTitle),
      );
      row.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen(row.dataset.srcUrl, row.dataset.srcTitle);
        }
      });
    });
  }
}

export function renderEvidenceValuation(el, state) {
  if (!el) return;
  const vals = [...state.valuations.values()];
  if (vals.length === 0 && !state.verification) {
    el.innerHTML = `<p class="dock-empty">Valuation summaries will land here after we build or compare a model.</p>`;
    return;
  }
  const blocks = vals
    .map((v) => {
      const items = [
        v.price_per_share != null
          ? `<span class="val-item"><span class="val-k">Implied</span><span class="val-v num">${escapeHtml(fmtNum(v.price_per_share))}</span></span>`
          : "",
        v.current_price != null
          ? `<span class="val-item"><span class="val-k">Current</span><span class="val-v num">${escapeHtml(fmtNum(v.current_price))}</span></span>`
          : "",
        v.upside_pct != null
          ? `<span class="val-item ${v.upside_pct >= 0 ? "up" : "down"}"><span class="val-k">Upside</span><span class="val-v num">${escapeHtml(fmtPct(v.upside_pct))}</span></span>`
          : "",
        v.ev != null
          ? `<span class="val-item"><span class="val-k">EV</span><span class="val-v num">${escapeHtml(fmtNum(v.ev))}M</span></span>`
          : "",
        v.wacc != null
          ? `<span class="val-item"><span class="val-k">WACC</span><span class="val-v num">${escapeHtml(fmtPct(v.wacc))}</span></span>`
          : "",
      ]
        .filter(Boolean)
        .join("");
      return `<section class="dock-val">
        <div class="dock-section-head num">${escapeHtml(v.ticker)}${v.currency ? ` · ${escapeHtml(v.currency)}` : ""}</div>
        ${items ? `<div class="val-strip">${items}</div>` : `<p class="card-note">Workbook built — add a share price for DCF upside.</p>`}
      </section>`;
    })
    .join("");
  const verify = state.verification
    ? `<p class="dock-verify status-${escapeHtml(state.verification.status)}">${escapeHtml(
        verifyStatusLabel(state.verification.status),
      )}${
        state.verification.verified != null && state.verification.total != null
          ? ` · ${state.verification.verified}/${state.verification.total} figures`
          : ""
      }</p>`
    : "";
  el.innerHTML = blocks + verify;
}

export function renderEvidenceArtifacts(el, state, { onOpen } = {}) {
  if (!el) return;
  if (state.artifacts.length === 0) {
    el.innerHTML = `<p class="dock-empty">Workbooks and decks we create will show up here.</p>`;
    return;
  }
  const rows = state.artifacts
    .map(
      (a) => `<li class="dock-artifact" data-art-path="${escapeHtml(a.path)}" role="button" tabindex="0">
      <span class="dock-artifact-kind">${a.kind === "deck" ? "Deck" : a.kind === "memo" ? "Memo" : "Workbook"}</span>
      <span class="dock-artifact-label">${escapeHtml(a.label)}</span>
      <span class="dock-artifact-open" aria-hidden="true">Open ↗</span>
    </li>`,
    )
    .join("");
  el.innerHTML = `<div class="dock-section-head">Artifacts · ${state.artifacts.length}</div>
    <ul class="dock-artifact-list">${rows}</ul>`;
  if (onOpen) {
    el.querySelectorAll("[data-art-path]").forEach((row) => {
      row.addEventListener("click", () => onOpen(row.dataset.artPath));
      row.addEventListener("keydown", (e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onOpen(row.dataset.artPath);
        }
      });
    });
  }
}

// ── live wiring (the chat feeds this; the dock renders itself) ───────

let liveState = createEvidenceState();

export function evidenceReset() {
  liveState = createEvidenceState();
  paint();
}

export function evidenceIngest(card) {
  if (evidenceAddCard(liveState, card)) paint();
}

function paint() {
  renderEvidenceSources($("dockPanel-sources"), liveState, {
    onOpen: (url, title) => {
      openDock("reader", { focusTab: false });
      openReader(url, title);
    },
  });
  renderEvidenceValuation($("dockPanel-valuation"), liveState);
  renderEvidenceArtifacts($("dockPanel-artifacts"), liveState, {
    onOpen: (path) => openPath(path),
  });
}
