// memory.mjs — MemoryUpdated notice + Undo window (Phase E UI).
//
// Pure reducer for the post-turn memory notice. Automatic capture shows
// "Memory updated · N" with 10-second Undo; Temporary Chat never emits notices.

/**
 * @typedef {object} MemoryNotice
 * @property {string} id
 * @property {number} count
 * @property {string[]} memoryIds
 * @property {string} [provenance]
 * @property {number} createdAt
 * @property {number} undoUntil
 * @property {boolean} undone
 */

/**
 * @typedef {object} MemoryUiState
 * @property {MemoryNotice|null} notice
 * @property {boolean} temporary
 * @property {MemoryNotice[]} history  recent notices (bounded)
 */

const UNDO_MS = 10_000;
const HISTORY_CAP = 20;

export function createMemoryUi() {
  return { notice: null, temporary: false, history: [] };
}

function clone(s) {
  return {
    notice: s.notice ? { ...s.notice, memoryIds: [...s.notice.memoryIds] } : null,
    temporary: s.temporary,
    history: s.history.map((n) => ({ ...n, memoryIds: [...n.memoryIds] })),
  };
}

function noticeLabel(n) {
  if (!n || n.undone) return "";
  const base = `Memory updated · ${n.count}`;
  return n.provenance ? `${base} · ${n.provenance}` : base;
}

export function reduce(state, action) {
  if (!action || !action.type) return state;

  switch (action.type) {
    case "SetTemporary": {
      const next = clone(state);
      next.temporary = !!action.value;
      if (next.temporary) next.notice = null;
      return next;
    }
    case "MemoryUpdated": {
      if (state.temporary) return state; // Temporary Chat captures nothing.
      const count = Number(action.count || action.memoryIds?.length || 0);
      if (!count) return state;
      const now = action.now ?? Date.now();
      const notice = {
        id: action.id || `mem-${now}`,
        count,
        memoryIds: [...(action.memoryIds || [])],
        provenance: action.provenance || "",
        createdAt: now,
        undoUntil: now + UNDO_MS,
        undone: false,
      };
      const next = clone(state);
      next.notice = notice;
      next.history = [notice, ...next.history].slice(0, HISTORY_CAP);
      return next;
    }
    case "UndoNotice": {
      if (!state.notice || state.notice.undone) return state;
      const now = action.now ?? Date.now();
      if (now > state.notice.undoUntil) return state; // window closed
      const next = clone(state);
      next.notice = { ...next.notice, undone: true };
      next.history = next.history.map((n) =>
        n.id === next.notice.id ? { ...n, undone: true } : n,
      );
      return next;
    }
    case "DismissNotice": {
      if (!state.notice) return state;
      const next = clone(state);
      next.notice = null;
      return next;
    }
    case "Tick": {
      // Drop expired undo affordance but keep the notice text until dismiss.
      if (!state.notice || state.notice.undone) return state;
      const now = action.now ?? Date.now();
      if (now <= state.notice.undoUntil) return state;
      return state; // callers use undoOpen() for UI; state stays until dismiss
    }
    default:
      return state;
  }
}

export function noticeText(state) {
  return noticeLabel(state.notice);
}

export function undoOpen(state, now = Date.now()) {
  const n = state.notice;
  return !!(n && !n.undone && now <= n.undoUntil);
}

/**
 * @param {HTMLElement|null} el
 * @param {MemoryUiState} state
 * @param {{ onUndo?: (notice: MemoryNotice) => void, onDismiss?: () => void, now?: number }} [hooks]
 */
export function render(el, state, hooks = {}) {
  if (!el) return;
  const n = state.notice;
  if (!n || n.undone) {
    el.hidden = true;
    el.innerHTML = "";
    return;
  }
  const now = hooks.now ?? Date.now();
  el.hidden = false;
  el.innerHTML = "";
  el.className = "memory-notice";
  el.setAttribute("role", "status");

  const text = document.createElement("span");
  text.className = "memory-notice-text";
  text.textContent = noticeLabel(n);
  el.appendChild(text);

  if (undoOpen(state, now)) {
    const undo = document.createElement("button");
    undo.type = "button";
    undo.className = "btn-ghost memory-undo";
    undo.textContent = "Undo";
    undo.addEventListener("click", () => hooks.onUndo?.(n));
    el.appendChild(undo);
  }

  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.className = "btn-ghost memory-dismiss";
  dismiss.setAttribute("aria-label", "Dismiss memory notice");
  dismiss.textContent = "Dismiss";
  dismiss.addEventListener("click", () => hooks.onDismiss?.());
  el.appendChild(dismiss);
}

export const UNDO_WINDOW_MS = UNDO_MS;
