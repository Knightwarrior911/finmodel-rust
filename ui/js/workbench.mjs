// workbench.mjs — owns the Evidence dock (Model / Valuation / Sources /
// Artifacts / Reader). One authority for: dock open/close/toggle, tab
// selection with a roving tablist, `body.dock-open`, focus return to the
// invoker, and the workbench keyboard map:
//   Ctrl/⌘+1..5  select a dock tab (opening the dock)
//   Ctrl/⌘+J     toggle the dock
//   ←/→/Home/End move between dock tabs while the tablist has focus
//   ↑/↓/Home/End move between plan steps while a plan step has focus
//   Esc          close the dock when focus is inside it and no run is active
// The global Ctrl/⌘+N, Ctrl/⌘+K, Ctrl/⌘+Enter, and run-stop Esc bindings stay
// in main.mjs / chat.mjs; this module never handles them.

const DOCK_TABS = ["model", "valuation", "sources", "artifacts", "reader"];

// Element the dock returns focus to when it closes (captured on open).
let dockReturnFocus = null;

function dockEl() {
  return document.getElementById("evidenceDock");
}
function tabBtn(tab) {
  return document.getElementById(`dockTab-${tab}`);
}
function panelEl(tab) {
  return document.getElementById(`dockPanel-${tab}`);
}

export function isDockOpen() {
  const d = dockEl();
  return !!d && !d.hidden;
}

export function activeDockTab() {
  for (const t of DOCK_TABS) {
    const b = tabBtn(t);
    if (b && b.getAttribute("aria-selected") === "true") return t;
  }
  return null;
}

/// Select a tab: roving tabindex + aria-selected on the tablist, show one
/// panel. Does not open/close the dock.
export function selectDockTab(tab) {
  if (!DOCK_TABS.includes(tab)) return;
  for (const t of DOCK_TABS) {
    const b = tabBtn(t);
    const p = panelEl(t);
    const on = t === tab;
    if (b) {
      b.setAttribute("aria-selected", on ? "true" : "false");
      b.tabIndex = on ? 0 : -1;
    }
    if (p) p.hidden = !on;
  }
}

/// Open the dock on `tab`. Captures the invoker for focus return the first
/// time it opens. `focusTab` moves keyboard focus onto the active tab.
export function openDock(tab = "model", { focusTab = true, returnFocus } = {}) {
  const d = dockEl();
  if (!d) return;
  if (!isDockOpen()) {
    const active =
      document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dockReturnFocus = returnFocus || active;
    d.hidden = false;
    document.body.classList.add("dock-open");
    // rAF so the slide-in transition runs from the hidden state.
    requestAnimationFrame(() => d.classList.add("open"));
  }
  const next = DOCK_TABS.includes(tab) ? tab : activeDockTab() || "model";
  selectDockTab(next);
  if (focusTab) {
    const b = tabBtn(activeDockTab());
    if (b) b.focus();
  }
}

export function closeDock() {
  const d = dockEl();
  if (!d) return;
  d.classList.remove("open");
  d.hidden = true;
  document.body.classList.remove("dock-open");
  const rf = dockReturnFocus;
  dockReturnFocus = null;
  if (rf && typeof rf.focus === "function" && document.contains(rf)) rf.focus();
}

export function toggleDock(tab = "model") {
  if (isDockOpen()) closeDock();
  else openDock(tab);
}

function moveTab(dir) {
  const cur = activeDockTab() || "model";
  let i = DOCK_TABS.indexOf(cur);
  if (dir === "home") i = 0;
  else if (dir === "end") i = DOCK_TABS.length - 1;
  else i = (i + (dir === "next" ? 1 : -1) + DOCK_TABS.length) % DOCK_TABS.length;
  const next = DOCK_TABS[i];
  selectDockTab(next);
  const b = tabBtn(next);
  if (b) b.focus();
}

function planSteps() {
  return Array.from(document.querySelectorAll(".plan-steps .plan-step"));
}

/// Roving focus across the live plan steps. Returns true when it moved.
function movePlan(dir) {
  const steps = planSteps();
  if (!steps.length) return false;
  let i = steps.indexOf(document.activeElement);
  if (i === -1) return false;
  if (dir === "home") i = 0;
  else if (dir === "end") i = steps.length - 1;
  else i = Math.min(steps.length - 1, Math.max(0, i + (dir === "next" ? 1 : -1)));
  steps.forEach((s, k) => (s.tabIndex = k === i ? 0 : -1));
  steps[i].focus();
  return true;
}

function isRunActive() {
  const stop = document.getElementById("chatStop");
  return !!stop && !stop.hidden;
}

export function initWorkbench() {
  const d = dockEl();
  if (!d) return;
  document.getElementById("dockClose")?.addEventListener("click", closeDock);
  for (const t of DOCK_TABS) {
    const b = tabBtn(t);
    if (b) b.addEventListener("click", () => selectDockTab(t));
  }
  // Roving tablist arrow navigation.
  d.querySelector(".dock-tabs")?.addEventListener("keydown", (e) => {
    switch (e.key) {
      case "ArrowRight":
      case "ArrowDown":
        e.preventDefault();
        moveTab("next");
        break;
      case "ArrowLeft":
      case "ArrowUp":
        e.preventDefault();
        moveTab("prev");
        break;
      case "Home":
        e.preventDefault();
        moveTab("home");
        break;
      case "End":
        e.preventDefault();
        moveTab("end");
        break;
    }
  });
  // Global workbench shortcuts + plan-step arrow navigation.
  document.addEventListener("keydown", (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && (e.key === "j" || e.key === "J")) {
      e.preventDefault();
      toggleDock();
      return;
    }
    if (mod && /^[1-5]$/.test(e.key)) {
      e.preventDefault();
      openDock(DOCK_TABS[Number(e.key) - 1]);
      return;
    }
    // Esc closes the dock only when focus is inside it and no run is running
    // (an active run keeps Esc as Stop, handled in main.mjs).
    if (e.key === "Escape" && isDockOpen() && d.contains(document.activeElement) && !isRunActive()) {
      e.preventDefault();
      closeDock();
      return;
    }
    // Plan-step roving navigation.
    const el = document.activeElement;
    if (el && el.classList && el.classList.contains("plan-step")) {
      if (e.key === "ArrowDown") {
        if (movePlan("next")) e.preventDefault();
      } else if (e.key === "ArrowUp") {
        if (movePlan("prev")) e.preventDefault();
      } else if (e.key === "Home") {
        if (movePlan("home")) e.preventDefault();
      } else if (e.key === "End") {
        if (movePlan("end")) e.preventDefault();
      }
    }
  });
}
