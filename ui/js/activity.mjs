// activity.mjs — Tool activity state reducer + renderer (Phase D).
//
// Reduces every agent event envelope into a keyed ToolActivity map
// (by tool_call_id). Renders status spinners, badges, expandable detail,
// batch trees, and approval prompts. Pure state reduction; no direct
// dependency on chat.mjs or conversation state.
//
// OMP anchors: tui/types.ts::State, tools/render-utils.ts, tui/tree-list.ts,
//             session/streaming-output.ts::TailBuffer
//
// Usage:
//   import { reduce, render, ActivityState } from "./activity.mjs";
//   // On each agent event:
//   const patches = reduce(state, event);
//   render(container, state, patches);

// ── Types ──────────────────────────────────────────────────────────────

/** @typedef {"queued"|"running"|"awaiting_approval"|"success"|"warning"|"error"|"cancelled"|"interrupted"} ToolStatus */

/**
 * @typedef {Object} ToolActivity
 * @property {string} tool_call_id
 * @property {string} name        - normalized tool name
 * @property {ToolStatus} status
 * @property {number} started_at  - epoch ms
 * @property {number|null} finished_at
 * @property {string|null} query  - sanitized user-visible input
 * @property {string|null} label  - human label
 * @property {string|null} detail - status detail line
 * @property {string|null} batch_id
 * @property {string|null} error
 * @property {Array<string>} tail - bounded output buffer
 * @property {boolean} expanded
 * @property {number} attempts
 */

/**
 * @typedef {Object} ActivityState
 * @property {Map<string, ToolActivity>} byId  - keyed by tool_call_id
 * @property {Map<string, string[]>} byBatch   - batch_id → [tool_call_id]
 * @property {Map<string, string>} parentOf   - tool_call_id → parent batch_id
 * @property {string|null} lastAnnounce
 */

/** Create a fresh empty state. */
export function createState() {
  return { byId: new Map(), byBatch: new Map(), parentOf: new Map(), lastAnnounce: null };
}

// ── Reducer ───────────────────────────────────────────────────────────

/**
 * Process one AgentEventEnvelope into state mutations.
 * Returns a list of tool_call_ids whose status changed.
 * @param {ActivityState} state
 * @param {Object} envelope  - { event, conversation_id, run_id, durability, … }
 * @returns {string[]} changed tool_call_ids
 */
export function reduce(state, envelope) {
  const ev = envelope.event || {};
  const changed = [];
  const key = (id) => id;

  switch (ev.type) {
    case "tool_started":
    case "ToolStarted": {
      const id = ev.tool_call_id || key(envelope.run_id + "_" + Date.now());
      const act = upsert(state, id);
      act.name = ev.name || "tool";
      act.status = "running";
      act.started_at = Date.now();
      act.finished_at = null;
      act.query = ev.query || ev.detail || null;
      act.label = ev.label || null;
      act.detail = ev.detail || null;
      act.batch_id = ev.batch_id || null;
      act.error = null;
      act.tail = [];
      act.expanded = false;
      act.attempts = (act.attempts || 0) + (ev.attempt !== undefined ? ev.attempt : 0);

      if (act.batch_id) {
        state.parentOf.set(id, act.batch_id);
        const batch = state.byBatch.get(act.batch_id) || [];
        if (!batch.includes(id)) batch.push(id);
        state.byBatch.set(act.batch_id, batch);
      }
      changed.push(id);
      break;
    }

    case "tool_progress":
    case "ToolProgress": {
      const id = ev.tool_call_id;
      if (!id || !state.byId.has(id)) break;
      const act = state.byId.get(id);
      if (act.tail.length < 6) {
        act.tail.push(ev.text || ev.detail || "");
      } else {
        // Bounded tail: drop earliest, push latest
        act.tail.shift();
        act.tail.push(ev.text || ev.detail || "");
      }
      act.detail = ev.detail || act.detail;
      changed.push(id);
      break;
    }

    case "tool_succeeded":
    case "ToolSucceeded": {
      const id = ev.tool_call_id;
      if (!id || !state.byId.has(id)) break;
      const act = state.byId.get(id);
      act.status = "success";
      act.finished_at = Date.now();
      act.detail = ev.summary || ev.detail || null;
      changed.push(id);
      break;
    }

    case "tool_warning":
    case "ToolWarning": {
      const id = ev.tool_call_id;
      if (!id || !state.byId.has(id)) break;
      const act = state.byId.get(id);
      act.status = "warning";
      act.detail = ev.detail || ev.summary || null;
      changed.push(id);
      break;
    }

    case "tool_failed":
    case "ToolFailed": {
      const id = ev.tool_call_id;
      if (!id || !state.byId.has(id)) break;
      const act = state.byId.get(id);
      act.status = "error";
      act.finished_at = Date.now();
      act.error = ev.error || ev.detail || null;
      act.detail = ev.detail || ev.summary || null;
      changed.push(id);
      break;
    }

    case "tool_cancelled":
    case "ToolCancelled": {
      const id = ev.tool_call_id;
      if (!id || !state.byId.has(id)) break;
      const act = state.byId.get(id);
      act.status = "cancelled";
      act.finished_at = Date.now();
      changed.push(id);
      break;
    }

    case "approval_requested":
    case "ApprovalRequested": {
      const id = ev.tool_call_id || key("approval_" + envelope.run_id);
      const act = upsert(state, id);
      act.status = "awaiting_approval";
      act.name = ev.name || "approval";
      act.label = ev.label || null;
      act.expanded = true;
      changed.push(id);
      break;
    }

    case "approval_resolved":
    case "ApprovalResolved": {
      const id = ev.tool_call_id;
      if (!id || !state.byId.has(id)) break;
      const act = state.byId.get(id);
      act.status = ev.response === "deny" ? "cancelled" : "success";
      act.finished_at = Date.now();
      act.detail = ev.response === "approve_once" ? "Approved" : "Denied";
      changed.push(id);
      break;
    }

    case "run_completed":
    case "RunCompleted":
    case "run_failed":
    case "RunFailed":
    case "run_cancelled":
    case "RunCancelled":
    case "run_interrupted":
    case "RunInterrupted":
    case "run_budget_limited":
    case "RunBudgetLimited": {
      // Terminal run events: mark any still-running activities as interrupted.
      for (const [id, act] of state.byId) {
        if (act.status === "running" || act.status === "queued") {
          act.status = "interrupted";
          act.finished_at = Date.now();
          changed.push(id);
        }
      }
      break;
    }
  }

  return changed;
}

function upsert(state, id) {
  if (state.byId.has(id)) return state.byId.get(id);
  const act = {
    tool_call_id: id,
    name: "tool",
    status: "queued",
    started_at: Date.now(),
    finished_at: null,
    query: null,
    label: null,
    detail: null,
    batch_id: null,
    error: null,
    tail: [],
    expanded: false,
    attempts: 0,
  };
  state.byId.set(id, act);
  return act;
}

// ── Render helpers ────────────────────────────────────────────────────

const STATUS_LABEL = {
  queued: "Queued",
  running: "Running",
  awaiting_approval: "Approval needed",
  success: "Done",
  warning: "Warning",
  error: "Error",
  cancelled: "Cancelled",
  interrupted: "Interrupted",
};

/** SVG spinner element (animated via CSS). */
function spinnerEl() {
  const s = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  s.setAttribute("class", "act-spinner");
  s.setAttribute("viewBox", "0 0 16 16");
  s.setAttribute("width", "14");
  s.setAttribute("height", "14");
  s.setAttribute("aria-hidden", "true");
  const c = document.createElementNS("http://www.w3.org/2000/svg", "circle");
  c.setAttribute("cx", "8");
  c.setAttribute("cy", "8");
  c.setAttribute("r", "6");
  c.setAttribute("fill", "none");
  c.setAttribute("stroke", "currentColor");
  c.setAttribute("stroke-width", "2");
  c.setAttribute("stroke-dasharray", "28");
  c.setAttribute("stroke-dashoffset", "20");
  s.appendChild(c);
  return s;
}

/** Status icon element (non-color-only for accessibility). */
function statusIcon(status) {
  if (status === "running" || status === "queued") return spinnerEl();
  const icons = {
    success: "✓",
    warning: "⚠",
    error: "✗",
    cancelled: "—",
    interrupted: "…",
    awaiting_approval: "?",
  };
  const span = document.createElement("span");
  span.className = `act-icon act-icon-${status}`;
  span.textContent = icons[status] || "•";
  span.setAttribute("aria-hidden", "true");
  return span;
}

/** Format elapsed ms → "3s" / "1m 12s" / "2m". */
function fmtDuration(ms) {
  if (ms == null) return "";
  const sec = Math.floor(ms / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  const rem = sec % 60;
  return rem ? `${min}m ${rem}s` : `${min}m`;
}

// ── Main render ───────────────────────────────────────────────────────

/**
 * Render (or patch) the activity container for the given tool call IDs.
 * Render only changed items when `ids` is provided; full render on empty.
 * @param {HTMLElement} container
 * @param {ActivityState} state
 * @param {string[]} [ids] - tool_call_ids that changed (partial update)
 */
export function render(container, state, ids) {
  if (!container) return;

  if (!ids || ids.length === 0) {
    // Full render.
    container.innerHTML = "";
    const els = new Map();
    for (const [id, act] of state.byId) {
      const el = renderActivity(act);
      container.appendChild(el);
      els.set(id, el);
    }
    container._els = els;
    return;
  }

  // Partial update: upsert only changed ids.
  const els = container._els || new Map();
  for (const id of ids) {
    const act = state.byId.get(id);
    if (!act) {
      const old = els.get(id);
      if (old) old.remove();
      els.delete(id);
      continue;
    }
    let el = els.get(id);
    if (el) {
      patchActivity(el, act);
    } else {
      el = renderActivity(act);
      container.appendChild(el);
      els.set(id, el);
    }
  }
  container._els = els;
}

function renderActivity(act) {
  const el = document.createElement("div");
  el.className = `act-row act-${act.status}`;
  el.dataset.toolCallId = act.tool_call_id;

  // Header row (always visible).
  const header = document.createElement("div");
  header.className = "act-header";

  header.appendChild(statusIcon(act.status));

  const nameSpan = document.createElement("span");
  nameSpan.className = "act-name";
  nameSpan.textContent = act.label || act.name;
  header.appendChild(nameSpan);

  if (act.query) {
    const q = document.createElement("span");
    q.className = "act-query";
    q.textContent = act.query;
    q.title = act.query;
    header.appendChild(q);
  }

  if (act.status === "running" || act.finished_at) {
    const dur = document.createElement("span");
    dur.className = "act-duration";
    dur.textContent = act.finished_at
      ? fmtDuration(act.finished_at - act.started_at)
      : fmtDuration(Date.now() - act.started_at);
    header.appendChild(dur);
  }

  const badge = document.createElement("span");
  badge.className = `act-badge act-badge-${act.status}`;
  badge.textContent = STATUS_LABEL[act.status] || act.status;
  header.appendChild(badge);

  if (act.status === "awaiting_approval") {
    const approve = document.createElement("button");
    approve.type = "button";
    approve.className = "act-btn act-btn-approve";
    approve.textContent = "Approve once";
    header.appendChild(approve);

    const deny = document.createElement("button");
    deny.type = "button";
    deny.className = "act-btn act-btn-deny";
    deny.textContent = "Deny";
    header.appendChild(deny);
  }

  el.appendChild(header);

  // Expandable detail area.
  const detail = document.createElement("div");
  detail.className = "act-detail";
  detail.hidden = !act.expanded && !act.error && act.tail.length === 0;

  if (act.error) {
    const err = document.createElement("p");
    err.className = "act-errtext";
    err.textContent = act.error;
    detail.appendChild(err);
  }

  if (act.detail && act.detail !== act.error) {
    const d = document.createElement("p");
    d.className = "act-detail-line";
    d.textContent = act.detail;
    detail.appendChild(d);
  }

  if (act.tail.length > 0) {
    const pre = document.createElement("pre");
    pre.className = "act-tail";
    pre.textContent = act.tail.join("\n");
    detail.appendChild(pre);
  }

  if (act.attempts > 1) {
    const att = document.createElement("span");
    att.className = "act-attempts";
    att.textContent = `Attempt ${act.attempts}`;
    detail.appendChild(att);
  }

  el.appendChild(detail);

  // Toggle expand.
  header.addEventListener("click", (e) => {
    // Don't toggle when clicking buttons.
    if (e.target.closest("button")) return;
    act.expanded = !act.expanded;
    detail.hidden = !act.expanded;
    el.classList.toggle("act-expanded", act.expanded);
  });

  return el;
}

function patchActivity(el, act) {
  el.className = `act-row act-${act.status}`;

  const header = el.querySelector(".act-header");
  if (header) {
    // Update duration.
    const dur = header.querySelector(".act-duration");
    if (dur) {
      dur.textContent = act.finished_at
        ? fmtDuration(act.finished_at - act.started_at)
        : fmtDuration(Date.now() - act.started_at);
    }

    // Update badge.
    const badge = header.querySelector(".act-badge");
    if (badge) badge.textContent = STATUS_LABEL[act.status] || act.status;
  }

  const detail = el.querySelector(".act-detail");
  if (detail) {
    detail.hidden = !act.expanded && !act.error && act.tail.length === 0;
    const errEl = detail.querySelector(".act-errtext");
    if (act.error) {
      if (errEl) errEl.textContent = act.error;
      else {
        p.className = "act-errtext";
        p.textContent = act.error;
        detail.insertBefore(p, detail.firstChild);
      }
    } else if (errEl) errEl.remove();

    const tailEl = detail.querySelector(".act-tail");
    if (act.tail.length > 0) {
      if (tailEl) tailEl.textContent = act.tail.join("\n");
      else {
        const pre = document.createElement("pre");
        pre.className = "act-tail";
        pre.textContent = act.tail.join("\n");
        detail.appendChild(pre);
      }
    } else if (tailEl) tailEl.remove();
  }
}

// ── Cleanup ────────────────────────────────────────────────────────────

/** Remove activities for a given run. */
export function cleanupRun(state, runId) {
  const removed = [];
  for (const [id, act] of state.byId) {
    if (id.startsWith(runId + "_") || act.batch_id?.startsWith(runId)) {
      state.byId.delete(id);
      removed.push(id);
    }
  }
  return removed;
}

/** Reset the entire state. */
export function resetState(state) {
  state.byId.clear();
  state.byBatch.clear();
  state.parentOf.clear();
  state.lastAnnounce = null;
}
