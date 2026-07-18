// main.mjs — boot: theme → reader → chat → sidebar → settings → update.

import { initTheme, call, on } from "./core.mjs";
import { initReader } from "./reader.mjs";
import { initWorkbench } from "./workbench.mjs";
import { initSidebar, refresh as refreshSidebar, setActive, getProjects } from "./sidebar.mjs";
import {
  initChat,
  loadConversation,
  newChat,
  getCurrentId,
  setModelPill,
  getActiveRunId,
  applyCapability,
  setPendingProjectId,
} from "./chat.mjs";
import { initSettings } from "./settings.mjs";
import { initUpdate } from "./update.mjs";
import { initAnalyst } from "./analyst.mjs";
import {
  createWorkspaceState,
  reduce as reduceWorkspace,
  render as renderWorkspace,
} from "./workspaces.mjs";
import { createTray, render as renderTray, reduce as reduceTray } from "./tasks.mjs";
import {
  createMemoryUi,
  reduce as reduceMemory,
  render as renderMemory,
} from "./memory.mjs";
import { initProjects, openProjectSettings, renderProjectDashboard } from "./projects.mjs";

async function loadModelPill() {
  try {
    const s = await call("load_settings");
    setModelPill(s.model);
    applyCapability(s);
  } catch (_) {
    /* offline */
  }
}

function boot() {
  initTheme();
  initWorkbench();
  initReader();
  initChat({
    onConversationChanged: () => {
      refreshSidebar();
      setActive(getCurrentId());
    },
  });
  const openProj = (id) => openProjectSettings(id, getProjects());
  initSidebar({
    onSelect: (id) => loadConversation(id),
    onNew: () => newChat(),
    onProjectSettings: openProj,
    onProjectOpen: async (proj) => {
      const all = await call("list_conversations").catch(() => []);
      renderProjectDashboard(
        proj,
        (all || []).filter((c) => c.project_id === proj.id),
      );
    },
  });
  initProjects({
    onChange: () => refreshSidebar(),
    onSettings: openProj,
    onNewChat: (pid) => {
      newChat();
      setPendingProjectId(pid);
      document.getElementById("chatInput")?.focus();
    },
    onOpenChat: (id) => loadConversation(id),
  });
  initSettings({ onSaved: () => loadModelPill() });
  initUpdate();
  initAnalyst();

  // Phase D chrome: workspace banner + empty task tray (populated once
  // agent_event streams land with agent_send).
  let workspaceState = createWorkspaceState();
  let memoryUi = createMemoryUi();
  let tray = createTray();
  const paintChrome = () => {
    renderWorkspace(
      {
        select: document.getElementById("workspaceSelect"),
        banner: document.getElementById("workspaceBanner"),
        tempBtn: document.getElementById("temporaryChatBtn"),
      },
      workspaceState,
      {
        onSelect: (id) => {
          workspaceState = reduceWorkspace(workspaceState, { type: "SelectWorkspace", id });
          paintChrome();
        },
        onToggleTemporary: () => {
          workspaceState = reduceWorkspace(workspaceState, { type: "ToggleTemporary" });
          memoryUi = reduceMemory(memoryUi, {
            type: "SetTemporary",
            value: workspaceState.temporary,
          });
          paintChrome();
        },
      },
    );
    renderTray(document.getElementById("taskTray"), tray, {
      onSelect: (id) => loadConversation(id),
      onCancel: () =>
        call("agent_cancel", { conversation_id: getCurrentId(), run_id: getActiveRunId() }).catch(
          () => {},
        ),
    });
    renderMemory(document.getElementById("memoryNotice"), memoryUi, {
      onUndo: () => {
        memoryUi = reduceMemory(memoryUi, { type: "UndoNotice" });
        paintChrome();
      },
      onDismiss: () => {
        memoryUi = reduceMemory(memoryUi, { type: "DismissNotice" });
        paintChrome();
      },
    });
  };
  paintChrome();

  // M4: surface parallel peer/company subagent fan-out in the task tray.
  on("agent_subagent", (e) => {
    const p = (e && e.payload) || {};
    if (p.sub_id === undefined || p.sub_id === null) return;
    tray = reduceTray(tray, {
      type: "SubagentUpdate",
      runId: `sub:${p.pool_id}:${p.sub_id}`,
      title: p.label,
      status: p.status,
      conversationId: p.conversation_id,
    });
    paintChrome();
  });

  // Global shortcuts: Ctrl/Cmd+N new chat, Ctrl/Cmd+K filter, Esc stops a reply.
  document.addEventListener("keydown", (e) => {
    const mod = e.ctrlKey || e.metaKey;
    if (mod && (e.key === "n" || e.key === "N")) {
      e.preventDefault();
      newChat();
      return;
    }
    if (mod && (e.key === "k" || e.key === "K")) {
      e.preventDefault();
      const f = document.getElementById("convFilter");
      if (f && !f.hidden) f.focus();
      return;
    }
    if (e.key === "Escape") {
      const stop = document.getElementById("chatStop");
      if (stop && !stop.hidden) {
        e.stopPropagation();
        call("agent_cancel", { conversation_id: getCurrentId(), run_id: getActiveRunId() }).catch(() => {});
      }
    }
  });
  refreshSidebar();
  loadModelPill();
  newChat();
}

boot();
