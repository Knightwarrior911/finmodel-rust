// sidebar.mjs — conversation history, new chat, rename/delete, collapse, theme.

import { $, call, escapeHtml, relTime, toggleTheme, currentTheme } from "./core.mjs";
import { getCurrentId } from "./chat.mjs";

let onSelect = () => {};
let onNew = () => {};
let allItems = [];

const SUN = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>`;
const MOON = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>`;
const PENCIL = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/></svg>`;
const TRASH = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>`;
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
    allItems = await call("list_conversations");
  } catch (_) {
    allItems = [];
  }
  const filter = $("convFilter");
  if (filter) filter.hidden = allItems.length < 6;
  applyFilter();
}

// Client-side substring filter over conversation titles.
function applyFilter() {
  const q = (($("convFilter") && $("convFilter").value) || "").trim().toLowerCase();
  const items = q ? allItems.filter((c) => (c.title || "").toLowerCase().includes(q)) : allItems;
  renderRows(items);
}

function renderRows(items) {
  const list = $("convList");
  const active = getCurrentId();
  if (!items.length) {
    list.innerHTML = `<p class="conv-empty">${allItems.length ? "No matches." : "No conversations yet."}</p>`;
    return;
  }
  // Non-interactive row container; a real <button> selects it (no nested
  // interactive controls). aria-current marks the open conversation (Phase 4.4).
  list.innerHTML = items
    .map((c) => {
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
          <button type="button" class="icon-btn conv-rename" data-id="${escapeHtml(c.id)}" aria-label="Rename conversation">${PENCIL}</button>
          <button type="button" class="icon-btn conv-delete" data-id="${escapeHtml(c.id)}" aria-label="Delete conversation">${TRASH}</button>
        </div>
      </div>`;
    })
    .join("");
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

  applyCollapsed(localStorage.getItem("sidebar") === "collapsed");
  updateThemeIcon();

  $("newChatBtn").addEventListener("click", () => onNew());

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
  // (Row selection is a native <button>; Enter/Space fire click automatically.)
  const convFilter = $("convFilter");
  if (convFilter) convFilter.addEventListener("input", applyFilter);
}
