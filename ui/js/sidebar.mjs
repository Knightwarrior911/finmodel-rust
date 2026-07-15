// sidebar.mjs — conversation history, new chat, rename/delete, collapse, theme.

import { $, call, escapeHtml, relTime, toggleTheme, currentTheme } from "./core.mjs";
import { getCurrentId } from "./chat.mjs";

let onSelect = () => {};
let onNew = () => {};

const SUN = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M2 12h2M20 12h2M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/></svg>`;
const MOON = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M21 12.8A9 9 0 1 1 11.2 3a7 7 0 0 0 9.8 9.8z"/></svg>`;
const PENCIL = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M12 20h9"/><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L7 19l-4 1 1-4z"/></svg>`;
const TRASH = `<svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6"/></svg>`;

function updateThemeIcon() {
  const btn = $("themeToggle");
  if (!btn) return;
  const dark = currentTheme() === "dark";
  btn.innerHTML = dark ? SUN : MOON;
  btn.setAttribute("aria-label", dark ? "Switch to light theme" : "Switch to dark theme");
}

export async function refresh() {
  const list = $("convList");
  let items = [];
  try {
    items = await call("list_conversations");
  } catch (_) {
    items = [];
  }
  const active = getCurrentId();
  if (!items.length) {
    list.innerHTML = `<p class="conv-empty">No conversations yet.</p>`;
    return;
  }
  list.innerHTML = items
    .map(
      (c) => `<div class="conv-row${c.id === active ? " active" : ""}" data-id="${escapeHtml(c.id)}" role="button" tabindex="0">
        <div class="conv-main">
          <span class="conv-title">${escapeHtml(c.title || "New conversation")}</span>
          <span class="conv-time num">${escapeHtml(relTime(c.updated))}</span>
        </div>
        <div class="conv-actions">
          <button type="button" class="icon-btn conv-rename" data-id="${escapeHtml(c.id)}" aria-label="Rename conversation">${PENCIL}</button>
          <button type="button" class="icon-btn conv-delete" data-id="${escapeHtml(c.id)}" aria-label="Delete conversation">${TRASH}</button>
        </div>
      </div>`
    )
    .join("");
}

export function setActive(id) {
  document.querySelectorAll("#convList .conv-row").forEach((row) => {
    row.classList.toggle("active", row.dataset.id === id);
  });
}

function beginRename(row) {
  const id = row.dataset.id;
  const titleEl = row.querySelector(".conv-title");
  const old = titleEl.textContent;
  const input = document.createElement("input");
  input.className = "conv-rename-input";
  input.value = old;
  titleEl.replaceWith(input);
  input.focus();
  input.select();
  const commit = async (save) => {
    const val = input.value.trim();
    input.replaceWith(titleEl);
    if (save && val && val !== old) {
      try {
        await call("rename_conversation", { id, title: val });
      } catch (_) {
        /* ignore */
      }
      await refresh();
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
      call("delete_conversation", { id: delBtn.dataset.id })
        .then(() => {
          if (getCurrentId() === delBtn.dataset.id) onNew();
          return refresh();
        })
        .catch(() => {});
      return;
    }
    const row = e.target.closest(".conv-row");
    if (row) onSelect(row.dataset.id);
  });
  $("convList").addEventListener("keydown", (e) => {
    if ((e.key === "Enter" || e.key === " ") && e.target.classList.contains("conv-row")) {
      e.preventDefault();
      onSelect(e.target.dataset.id);
    }
  });
}
