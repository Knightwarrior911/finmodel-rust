// workspaces.mjs — Workspace chrome state (Phase D).
//
// Pure reducer for workspace selection, confidentiality banner, and Temporary
// Chat mode. No DOM I/O beyond an optional render helper.

/** @typedef {"standard"|"confidential"|"restricted"} Confidentiality */

/**
 * @typedef {object} Workspace
 * @property {string} id
 * @property {string} name
 * @property {string} kind
 * @property {Confidentiality} confidentiality
 * @property {boolean} memoryEnabled
 */

/**
 * @typedef {object} WorkspaceState
 * @property {Workspace[]} list
 * @property {string|null} activeId
 * @property {boolean} temporary
 */

export function createWorkspaceState() {
  return {
    list: [
      {
        id: "personal",
        name: "Personal",
        kind: "personal",
        confidentiality: "standard",
        memoryEnabled: true,
      },
    ],
    activeId: "personal",
    temporary: false,
  };
}

function clone(s) {
  return {
    list: s.list.map((w) => ({ ...w })),
    activeId: s.activeId,
    temporary: s.temporary,
  };
}

export function reduce(state, action) {
  if (!action || !action.type) return state;
  switch (action.type) {
    case "SetWorkspaces": {
      const next = clone(state);
      next.list = (action.workspaces || []).map((w) => ({
        id: w.id,
        name: w.name || w.id,
        kind: w.kind || "deal",
        confidentiality: w.confidentiality || "confidential",
        memoryEnabled: w.memoryEnabled !== false,
      }));
      if (!next.list.find((w) => w.id === next.activeId)) {
        next.activeId = next.list[0]?.id ?? null;
      }
      return next;
    }
    case "SelectWorkspace": {
      if (!state.list.find((w) => w.id === action.id)) return state;
      const next = clone(state);
      next.activeId = action.id;
      // Leaving Temporary Chat destroys its in-memory session semantics.
      next.temporary = false;
      return next;
    }
    case "ToggleTemporary": {
      const next = clone(state);
      next.temporary = action.value ?? !state.temporary;
      return next;
    }
    case "SetConfidentiality": {
      const next = clone(state);
      const w = next.list.find((x) => x.id === (action.id || next.activeId));
      if (!w) return state;
      if (
        !["standard", "confidential", "restricted"].includes(
          action.confidentiality,
        )
      ) {
        return state;
      }
      w.confidentiality = action.confidentiality;
      return next;
    }
    default:
      return state;
  }
}

export function activeWorkspace(state) {
  return state.list.find((w) => w.id === state.activeId) || null;
}

/** Banner copy for the active workspace / Temporary mode. */
export function bannerText(state) {
  if (state.temporary) {
    return "Temporary Chat — nothing is saved, recalled, or captured.";
  }
  const w = activeWorkspace(state);
  if (!w) return "";
  if (w.confidentiality === "restricted") {
    return `${w.name} · Restricted — provider egress needs per-turn approval.`;
  }
  if (w.confidentiality === "confidential") {
    return `${w.name} · Confidential — deal facts stay in this workspace.`;
  }
  return "";
}

/**
 * Render workspace chrome into known nodes.
 * @param {{ select?: HTMLSelectElement|null, banner?: HTMLElement|null, tempBtn?: HTMLElement|null }} els
 * @param {WorkspaceState} state
 * @param {{ onSelect?: (id: string) => void, onToggleTemporary?: () => void }} [hooks]
 */
export function render(els, state, hooks = {}) {
  const { select, banner, tempBtn } = els || {};
  if (select) {
    select.innerHTML = "";
    for (const w of state.list) {
      const opt = document.createElement("option");
      opt.value = w.id;
      opt.textContent = w.name;
      if (w.id === state.activeId) opt.selected = true;
      select.appendChild(opt);
    }
    select.onchange = () => hooks.onSelect?.(select.value);
  }
  if (banner) {
    const text = bannerText(state);
    banner.textContent = text;
    banner.hidden = !text;
    banner.dataset.tier = state.temporary
      ? "temporary"
      : activeWorkspace(state)?.confidentiality || "";
  }
  if (tempBtn) {
    tempBtn.setAttribute("aria-pressed", state.temporary ? "true" : "false");
    tempBtn.classList.toggle("is-active", state.temporary);
    tempBtn.textContent = state.temporary
      ? "Temporary chat · on"
      : "Temporary chat";
    tempBtn.onclick = () => hooks.onToggleTemporary?.();
  }
}
