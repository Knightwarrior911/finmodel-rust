// sidebar.mjs — conversation history, new chat, rename/delete, collapse, theme.

import { $, call, escapeHtml, relTime, toggleTheme, currentTheme } from "./core.mjs";
import { getCurrentId } from "./chat.mjs";

let onSelect = () => {};
let onNew = () => {};
let allItems = [];
let projects = [];
let onProjectSettings = () => {};
let onProjectOpen = () => {};
const collapsedFolders = new Set(
  (() => {
    try {
      return JSON.parse(localStorage.getItem("foldersCollapsed") || "[]");
    } catch (_) {
      return [];
    }
  })(),
);
function persistCollapsed() {
  localStorage.setItem("foldersCollapsed", JSON.stringify([...collapsedFolders]));
}

const SUN = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>`;
const MOON = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>`;
const PENCIL = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/></svg>`;
const TRASH = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>`;
const FOLDER = `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>`;
const GEAR = `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 1 1-2.83 2.83l-.06-.06a1.65 1.65 0 0 0-1.82-.33 1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-4 0v-.09A1.65 1.65 0 0 0 8 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 1 1-2.83-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H2a2 2 0 0 1 0-4h.09A1.65 1.65 0 0 0 3.6 8a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 1 1 2.83-2.83l.06.06a1.65 1.65 0 0 0 1.82.33h.09A1.65 1.65 0 0 0 8 3.6V2a2 2 0 0 1 4 0v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 1 1 2.83 2.83l-.06.06a1.65 1.65 0 0 0-.33 1.82v.09a1.65 1.65 0 0 0 1.51 1H22a2 2 0 0 1 0 4h-.09a1.65 1.65 0 0 0-1.51 1z"/></svg>`;
const MOVE = `<svg width="15" height="15" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/><path d="M12 10.5v5M9.5 13l2.5 2.5 2.5-2.5"/></svg>`;
const CHECK = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M20 6L9 17l-5-5"/></svg>`;
const CROSS = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6L6 18M6 6l12 12"/></svg>`;

function updateThemeIcon() {
  const btn = $("themeToggle");
  if (!btn) return;
  const dark = currentTheme() === "dark";
  btn.innerHTML = dark ? SUN : MOON;
  btn.setAttribute("aria-label", dark ? "Switch to light theme" : "Switch to dark theme");
}

/// Announce a sidebar failure with a keyboard-reachable Retry / Dismiss. Keeps
/// the list intact (never silently discards selection). Phase 4.3.
function announce(message, onRetry) {
  const a = $("sidebarAlert");
  if (!a) return;
  a.innerHTML = "";
  const note = document.createElement("span");
  note.textContent = message;
  a.appendChild(note);
  if (onRetry) {
    const retry = document.createElement("button");
    retry.type = "button";
    retry.className = "btn-ghost";
    retry.textContent = "Retry";
    retry.addEventListener("click", () => {
      a.hidden = true;
      a.innerHTML = "";
      onRetry();
    });
    a.appendChild(retry);
  }
  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.className = "btn-ghost";
  dismiss.textContent = "Dismiss";
  dismiss.addEventListener("click", () => {
    a.hidden = true;
    a.innerHTML = "";
  });
  a.appendChild(dismiss);
  a.hidden = false;
}

export async function refresh() {
  try {
    const [convs, projs] = await Promise.all([
      call("list_conversations"),
      call("projects_list").catch(() => []),
    ]);
    allItems = convs;
    projects = projs || [];
  } catch (_) {
    allItems = [];
  }
  const filter = $("convFilter");
  if (filter) filter.hidden = allItems.length < 6;
  applyFilter();
}

/// Projects (folders) in the active workspace, for the move menu + dashboard.
export function getProjects() {
  return projects;
}

// Client-side substring filter over conversation titles.
function applyFilter() {
  const q = (($("convFilter") && $("convFilter").value) || "").trim().toLowerCase();
  const items = q ? allItems.filter((c) => (c.title || "").toLowerCase().includes(q)) : allItems;
  renderRows(items);
}

function rowHtml(c, active) {
  const isActive = c.id === active;
  const title = escapeHtml(c.title || "New conversation");
  return `<div class="conv-row${isActive ? " active" : ""}" data-id="${escapeHtml(c.id)}">
    <button type="button" class="conv-open" data-id="${escapeHtml(c.id)}"${
      isActive ? ' aria-current="true"' : ""
    }>
      <span class="conv-title">${title}</span>
      <span class="conv-time num">${escapeHtml(relTime(c.updated))}</span>
    </button>
    <div class="conv-actions">
      <button type="button" class="icon-btn conv-move" data-id="${escapeHtml(c.id)}" aria-label="Move to project" title="Move to project">${MOVE}</button>
      <button type="button" class="icon-btn conv-rename" data-id="${escapeHtml(c.id)}" aria-label="Rename conversation">${PENCIL}</button>
      <button type="button" class="icon-btn conv-delete" data-id="${escapeHtml(c.id)}" aria-label="Delete conversation">${TRASH}</button>
    </div>
  </div>`;
}

function renderRows(items) {
  const list = $("convList");
  const active = getCurrentId();
  if (!items.length && !projects.length) {
    list.innerHTML = `<p class="conv-empty">${allItems.length ? "No matches." : "No conversations yet."}</p>`;
    return;
  }
  const byProject = new Map();
  const loose = [];
  for (const c of items) {
    if (c.project_id) {
      if (!byProject.has(c.project_id)) byProject.set(c.project_id, []);
      byProject.get(c.project_id).push(c);
    } else {
      loose.push(c);
    }
  }
  let html = "";
  for (const p of projects) {
    const chats = byProject.get(p.id) || [];
    const collapsed = collapsedFolders.has(p.id);
    html += `<div class="proj-folder${collapsed ? " collapsed" : ""}" data-project="${escapeHtml(p.id)}">
      <div class="proj-head">
        <button type="button" class="proj-toggle" data-project="${escapeHtml(p.id)}" aria-expanded="${
          collapsed ? "false" : "true"
        }" aria-label="Toggle folder"><span class="proj-caret" aria-hidden="true">▾</span></button>
        <button type="button" class="proj-open" data-project="${escapeHtml(p.id)}">
          <span class="proj-fold-ic" aria-hidden="true">${FOLDER}</span><span class="proj-name">${escapeHtml(p.name)}</span>
          <span class="proj-count num">${chats.length}</span>
        </button>
        <button type="button" class="icon-btn proj-gear" data-project="${escapeHtml(p.id)}" aria-label="Project settings" title="Project settings">${GEAR}</button>
      </div>
      <div class="proj-chats">${
        chats.map((c) => rowHtml(c, active)).join("") ||
        '<p class="conv-empty proj-empty">Empty folder.</p>'
      }</div>
    </div>`;
  }
  if (loose.length) html += loose.map((c) => rowHtml(c, active)).join("");
  list.innerHTML = html || `<p class="conv-empty">No conversations yet.</p>`;
}

export function setActive(id) {
  document.querySelectorAll("#convList .conv-row").forEach((row) => {
    const on = row.dataset.id === id;
    row.classList.toggle("active", on);
    const open = row.querySelector(".conv-open");
    if (open) {
      if (on) open.setAttribute("aria-current", "true");
      else open.removeAttribute("aria-current");
    }
  });
}

function beginRename(row) {
  const id = row.dataset.id;
  const openBtn = row.querySelector(".conv-open");
  const old = (openBtn.querySelector(".conv-title") || {}).textContent || "";
  const input = document.createElement("input");
  input.className = "conv-rename-input";
  input.value = old;
  input.setAttribute("aria-label", "Rename conversation");
  // Replace the whole select button (no input nested in a button).
  openBtn.hidden = true;
  openBtn.after(input);
  input.focus();
  input.select();
  const commit = async (save) => {
    const val = input.value.trim();
    input.remove();
    openBtn.hidden = false;
    if (save && val && val !== old) {
      try {
        await call("rename_conversation", { id, title: val });
        await refresh();
      } catch (e) {
        // Retain the old title; announce + offer retry (Phase 4.3).
        announce((e && e.message) || "Rename failed.", () => beginRename(row));
      }
    }
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") { e.preventDefault(); commit(true); }
    else if (e.key === "Escape") { e.preventDefault(); commit(false); }
  });
  input.addEventListener("blur", () => commit(true));
}

function applyCollapsed(collapsed) {
  document.body.classList.toggle("sidebar-collapsed", collapsed);
  const toggle = $("sidebarToggle");
  const sidebar = $("sidebar");
  if (toggle) toggle.setAttribute("aria-expanded", collapsed ? "false" : "true");
  // Collapsed descendants are inert (not focusable/announced); the floating
  // expand button lives outside #sidebar so it stays reachable.
  if (sidebar) {
    if (collapsed) sidebar.setAttribute("inert", "");
    else sidebar.removeAttribute("inert");
  }
}

export function initSidebar(opts = {}) {
  onSelect = opts.onSelect || (() => {});
  onNew = opts.onNew || (() => {});
  onProjectSettings = opts.onProjectSettings || (() => {});
  onProjectOpen = opts.onProjectOpen || (() => {});

  applyCollapsed(localStorage.getItem("sidebar") === "collapsed");
  updateThemeIcon();

  $("newChatBtn").addEventListener("click", () => onNew());
  const newProjBtn = $("newProjectBtn");
  if (newProjBtn)
    newProjBtn.addEventListener("click", async () => {
      try {
        const res = await call("project_create", { name: "New project" });
        const proj = typeof res === "string" ? JSON.parse(res) : res;
        await refresh();
        if (proj && proj.id) onProjectSettings(proj.id);
      } catch (_) {
        /* ignore create failure */
      }
    });

  $("sidebarToggle").addEventListener("click", () => {
    const collapsed = !document.body.classList.contains("sidebar-collapsed");
    applyCollapsed(collapsed);
    localStorage.setItem("sidebar", collapsed ? "collapsed" : "open");
  });
  const expand = $("sidebarExpand");
  if (expand)
    expand.addEventListener("click", () => {
      applyCollapsed(false);
      localStorage.setItem("sidebar", "open");
    });

  $("themeToggle").addEventListener("click", () => {
    toggleTheme();
    updateThemeIcon();
    document.dispatchEvent(new CustomEvent("theme-changed"));
  });
  document.addEventListener("theme-changed", updateThemeIcon);

  $("convList").addEventListener("click", (e) => {
    const projToggle = e.target.closest(".proj-toggle");
    if (projToggle) {
      e.stopPropagation();
      const pid = projToggle.dataset.project;
      if (collapsedFolders.has(pid)) collapsedFolders.delete(pid);
      else collapsedFolders.add(pid);
      persistCollapsed();
      applyFilter();
      return;
    }
    const projGear = e.target.closest(".proj-gear");
    if (projGear) {
      e.stopPropagation();
      onProjectSettings(projGear.dataset.project);
      return;
    }
    const projOpen = e.target.closest(".proj-open");
    if (projOpen) {
      e.stopPropagation();
      const pid = projOpen.dataset.project;
      onProjectOpen(projects.find((x) => x.id === pid) || { id: pid, name: "" });
      return;
    }
    const moveBtn = e.target.closest(".conv-move");
    if (moveBtn) {
      e.stopPropagation();
      const actions = moveBtn.closest(".conv-actions");
      const cid = moveBtn.dataset.id;
      const opts2 = ['<option value="">— No project —</option>']
        .concat(
          projects.map((p) => `<option value="${escapeHtml(p.id)}">${escapeHtml(p.name)}</option>`),
        )
        .join("");
      actions.innerHTML = `<select class="conv-move-sel" data-id="${escapeHtml(cid)}" aria-label="Move to project">${opts2}</select>`;
      const sel = actions.querySelector("select");
      if (sel) sel.focus();
      return;
    }
    const renameBtn = e.target.closest(".conv-rename");
    if (renameBtn) {
      e.stopPropagation();
      beginRename(renameBtn.closest(".conv-row"));
      return;
    }
    const delBtn = e.target.closest(".conv-delete");
    if (delBtn) {
      e.stopPropagation();
      const actions = delBtn.closest(".conv-actions");
      actions.innerHTML = `<button type="button" class="icon-btn conv-del-yes" data-id="${escapeHtml(delBtn.dataset.id)}" aria-label="Confirm delete">${CHECK}</button><button type="button" class="icon-btn conv-del-no" aria-label="Cancel delete">${CROSS}</button>`;
      setTimeout(() => refresh(), 3000);
      return;
    }
    const yesBtn = e.target.closest(".conv-del-yes");
    if (yesBtn) {
      e.stopPropagation();
      const delId = yesBtn.dataset.id;
      call("delete_conversation", { id: delId })
        .then(() => {
          if (getCurrentId() === delId) onNew();
          return refresh();
        })
        .catch((err) => {
          announce((err && err.message) || "Delete failed.", () => {
            call("delete_conversation", { id: delId })
              .then(() => {
                if (getCurrentId() === delId) onNew();
                return refresh();
              })
              .catch(() => {});
          });
        });
      return;
    }
    const noBtn = e.target.closest(".conv-del-no");
    if (noBtn) {
      e.stopPropagation();
      refresh();
      return;
    }
    const row = e.target.closest(".conv-row");
    if (row) onSelect(row.dataset.id);
  });
  $("convList").addEventListener("change", (e) => {
    const sel = e.target.closest(".conv-move-sel");
    if (!sel) return;
    const cid = sel.dataset.id;
    const pid = sel.value || null;
    call("conversation_set_project", { conversation_id: cid, project_id: pid })
      .then(() => refresh())
      .catch(() => refresh());
  });
  // (Row selection is a native <button>; Enter/Space fire click automatically.)
  const convFilter = $("convFilter");
  if (convFilter) convFilter.addEventListener("input", applyFilter);
}
