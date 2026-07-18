// projects.mjs — project (folder) settings modal + center dashboard view.
//
// The modal edits a project's name (project_rename) and its grounding
// instructions (grounding_set_project → projects/<id>/finmodel.md), and can
// delete the project. The dashboard renders into #chatScroll when a folder is
// opened from the sidebar: project name, chat list, and a "new chat in project"
// action. All backend access is via Tauri commands.

import { $, call, escapeHtml } from "./core.mjs";

let currentProjectId = null;
let hooks = {
  onChange: () => {},
  onSettings: () => {},
  onNewChat: () => {},
  onOpenChat: () => {},
};

function status(msg, kind) {
  const el = $("projectStatus");
  if (!el) return;
  el.hidden = false;
  el.textContent = msg;
  el.className = `status ${kind || "info"}`;
}

function closeModal() {
  const m = $("projectModal");
  if (m) m.hidden = true;
  currentProjectId = null;
}

/// Open the settings + grounding modal for a project.
export async function openProjectSettings(projectId, projects) {
  currentProjectId = projectId;
  const proj = (projects || []).find((p) => p.id === projectId) || { id: projectId, name: "" };
  $("projectName").value = proj.name || "";
  const st = $("projectStatus");
  if (st) st.hidden = true;
  let instr = "";
  try {
    instr = await call("grounding_get_project", { project_id: projectId });
  } catch (_) {
    /* leave blank */
  }
  $("projectInstructions").value = instr || "";
  $("projectModal").hidden = false;
  setTimeout(() => $("projectName").focus(), 30);
}

/// Render the project dashboard into the center scroll area.
export function renderProjectDashboard(project, chats) {
  const scroll = $("chatScroll");
  if (!scroll) return;
  const rows =
    (chats || [])
      .map(
        (c) =>
          `<button type="button" class="dash-chat" data-id="${escapeHtml(c.id)}">${escapeHtml(
            c.title || "New conversation",
          )}</button>`,
      )
      .join("") || `<p class="dash-empty">No chats yet in this project.</p>`;
  const n = (chats || []).length;
  scroll.innerHTML = `
    <section class="project-dashboard">
      <div class="dash-head">
        <h1 class="dash-title">${escapeHtml(project.name || "Project")}</h1>
        <button type="button" class="btn-ghost" id="dashSettings" data-id="${escapeHtml(project.id)}">Settings &amp; grounding</button>
      </div>
      <p class="dash-sub">${n} chat${n === 1 ? "" : "s"} · shared grounding applies to every chat in this folder.</p>
      <div class="dash-chats">${rows}</div>
      <button type="button" class="btn-primary dash-new" id="dashNewChat" data-id="${escapeHtml(project.id)}">+ New chat in project</button>
    </section>`;
}

export function initProjects(opts = {}) {
  hooks = { ...hooks, ...opts };

  const modal = $("projectModal");
  $("projectClose")?.addEventListener("click", closeModal);
  modal?.querySelector(".modal-backdrop")?.addEventListener("click", closeModal);

  $("projectSave")?.addEventListener("click", async () => {
    if (!currentProjectId) return;
    const name = $("projectName").value.trim();
    const instructions = $("projectInstructions").value;
    try {
      if (name) await call("project_rename", { id: currentProjectId, name });
      await call("grounding_set_project", { project_id: currentProjectId, instructions });
      status("Saved.", "ok");
      hooks.onChange();
      setTimeout(closeModal, 400);
    } catch (e) {
      status((e && e.message) || "Save failed.", "error");
    }
  });

  $("projectDelete")?.addEventListener("click", async () => {
    if (!currentProjectId) return;
    try {
      await call("project_delete", { id: currentProjectId });
      hooks.onChange();
      closeModal();
    } catch (e) {
      status((e && e.message) || "Delete failed.", "error");
    }
  });

  // Dashboard delegated clicks (elements exist only while a dashboard is shown).
  $("chatScroll")?.addEventListener("click", (e) => {
    const setBtn = e.target.closest("#dashSettings");
    if (setBtn) {
      hooks.onSettings(setBtn.dataset.id);
      return;
    }
    const newBtn = e.target.closest("#dashNewChat");
    if (newBtn) {
      hooks.onNewChat(newBtn.dataset.id);
      return;
    }
    const chatBtn = e.target.closest(".dash-chat");
    if (chatBtn) {
      hooks.onOpenChat(chatBtn.dataset.id);
    }
  });
}
