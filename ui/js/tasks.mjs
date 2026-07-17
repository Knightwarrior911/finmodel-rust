// tasks.mjs — Non-blocking task tray state (Phase D).
//
// Tracks up to three concurrent conversation runs for the tray UI. Pure
// reducer — no DOM. The tray shows background work when the user switches
// conversations.

/** @typedef {"queued"|"running"|"awaiting_approval"|"completed"|"failed"|"cancelled"|"interrupted"} TaskStatus */

/**
 * @typedef {object} Task
 * @property {string} conversationId
 * @property {string} runId
 * @property {string} title
 * @property {TaskStatus} status
 * @property {string} [phase]
 * @property {number} updatedAt
 */

/**
 * @typedef {object} TaskTray
 * @property {Map<string, Task>} byRun  keyed by runId
 * @property {string|null} focusedConversationId
 */

export function createTray() {
  return { byRun: new Map(), focusedConversationId: null };
}

function keyOf(env) {
  return env?.run_id || env?.runId || null;
}

function kindOf(env) {
  const k = env?.event?.kind || env?.event?.type || env?.type || "";
  return String(k);
}

function clone(tray) {
  return {
    byRun: new Map(tray.byRun),
    focusedConversationId: tray.focusedConversationId,
  };
}

/**
 * Reduce an AgentEventEnvelope (or a tray-local action) into a new tray.
 * Unknown events return the same reference.
 */
export function reduce(tray, env) {
  if (!env) return tray;
  if (env.type === "FocusConversation") {
    const next = clone(tray);
    next.focusedConversationId = env.conversationId ?? null;
    return next;
  }
  if (env.type === "DismissTask") {
    const next = clone(tray);
    next.byRun.delete(env.runId);
    return next;
  }
  if (env.type === "SubagentUpdate") {
    const next = clone(tray);
    const map = { running: "running", done: "completed", error: "failed", cancelled: "cancelled" };
    next.byRun.set(env.runId, {
      conversationId: env.conversationId ?? "",
      runId: env.runId,
      title: env.title || "task",
      status: map[env.status] || "running",
      phase: "subagent",
      updatedAt: Date.now(),
    });
    return next;
  }

  const runId = keyOf(env);
  if (!runId) return tray;
  const kind = kindOf(env);
  const conversationId =
    env.conversation_id || env.conversationId || tray.byRun.get(runId)?.conversationId || "";
  const title =
    env.event?.payload?.title ||
    env.title ||
    tray.byRun.get(runId)?.title ||
    "Untitled run";
  const now = Date.now();

  /** @type {TaskStatus|null} */
  let status = null;
  let phase = tray.byRun.get(runId)?.phase;
  switch (kind) {
    case "RunStarted":
    case "run_started":
      status = "running";
      phase = "preparing";
      break;
    case "PhaseChanged":
    case "phase_changed":
      status = "running";
      phase = env.event?.payload?.phase || env.detail || phase;
      break;
    case "ApprovalRequested":
    case "approval_requested":
      status = "awaiting_approval";
      break;
    case "ApprovalResolved":
    case "approval_resolved":
      status = "running";
      break;
    case "RunCompleted":
    case "run_completed":
      status = "completed";
      break;
    case "RunFailed":
    case "run_failed":
      status = "failed";
      break;
    case "RunCancelled":
    case "run_cancelled":
      status = "cancelled";
      break;
    case "RunInterrupted":
    case "run_interrupted":
      status = "interrupted";
      break;
    default:
      return tray;
  }

  const next = clone(tray);
  next.byRun.set(runId, {
    conversationId,
    runId,
    title,
    status,
    phase,
    updatedAt: now,
  });
  return next;
}

/** Active (non-terminal) tasks, newest first. */
export function activeTasks(tray) {
  const terminal = new Set(["completed", "failed", "cancelled", "interrupted"]);
  return [...tray.byRun.values()]
    .filter((t) => !terminal.has(t.status))
    .sort((a, b) => b.updatedAt - a.updatedAt);
}

/** Background tasks: active runs whose conversation is not focused. */
export function backgroundTasks(tray) {
  const focus = tray.focusedConversationId;
  return activeTasks(tray).filter((t) => t.conversationId !== focus);
}

/** Cap display to three rows (product concurrency limit). */
export function visibleTasks(tray) {
  return activeTasks(tray).slice(0, 3);
}

/**
 * Render the tray into `el`. Returns the number of visible rows.
 * @param {HTMLElement} el
 * @param {TaskTray} tray
 * @param {{ onSelect?: (conversationId: string, runId: string) => void, onCancel?: (conversationId: string, runId: string) => void }} [hooks]
 */
export function render(el, tray, hooks = {}) {
  if (!el) return 0;
  const rows = visibleTasks(tray);
  const bg = new Set(backgroundTasks(tray).map((t) => t.runId));
  el.innerHTML = "";
  el.hidden = rows.length === 0;
  for (const t of rows) {
    const row = document.createElement("div");
    row.className = `task-row task-${t.status}${bg.has(t.runId) ? " task-bg" : ""}`;
    row.dataset.runId = t.runId;
    row.dataset.conversationId = t.conversationId;
    row.setAttribute("role", "listitem");

    const label = document.createElement("button");
    label.type = "button";
    label.className = "task-label";
    label.textContent = t.title;
    label.title = `${t.status}${t.phase ? ` · ${t.phase}` : ""}`;
    label.addEventListener("click", () => hooks.onSelect?.(t.conversationId, t.runId));
    row.appendChild(label);

    const badge = document.createElement("span");
    badge.className = "task-badge";
    badge.textContent = t.status.replaceAll("_", " ");
    row.appendChild(badge);

    if (t.status === "running" || t.status === "awaiting_approval" || t.status === "queued") {
      const cancel = document.createElement("button");
      cancel.type = "button";
      cancel.className = "task-cancel btn-ghost";
      cancel.setAttribute("aria-label", `Cancel ${t.title}`);
      cancel.textContent = "Stop";
      cancel.addEventListener("click", (e) => {
        e.stopPropagation();
        hooks.onCancel?.(t.conversationId, t.runId);
      });
      row.appendChild(cancel);
    }
    el.appendChild(row);
  }
  return rows.length;
}
