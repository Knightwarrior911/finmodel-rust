// main.mjs — boot: theme → reader → chat → sidebar → settings → update.

import { initTheme, call } from "./core.mjs";
import { initReader } from "./reader.mjs";
import { initSidebar, refresh as refreshSidebar, setActive } from "./sidebar.mjs";
import {
  initChat,
  loadConversation,
  newChat,
  getCurrentId,
  setModelPill,
  getActiveRunId,
  applyCapability,
} from "./chat.mjs";
import { initSettings } from "./settings.mjs";
import { initUpdate } from "./update.mjs";
import { initAnalyst } from "./analyst.mjs";
import {
  createWorkspaceState,
  reduce as reduceWorkspace,
  render as renderWorkspace,
} from "./workspaces.mjs";
import { createTray, render as renderTray } from "./tasks.mjs";
import {
  createMemoryUi,
  reduce as reduceMemory,
  render as renderMemory,
} from "./memory.mjs";

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
  initReader();
  initChat({
    onConversationChanged: () => {
      refreshSidebar();
      setActive(getCurrentId());
    },
  });
  initSidebar({
    onSelect: (id) => loadConversation(id),
    onNew: () => newChat(),
  });
  initSettings({ onSaved: () => loadModelPill() });
  initUpdate();
  initAnalyst();

  // Phase D chrome: workspace banner + empty task tray (populated once
  // agent_event streams land with agent_send).
  let workspaceState = createWorkspaceState();
  let memoryUi = createMemoryUi();
  const tray = createTray();
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
        call("chat_cancel", { conversation_id: getCurrentId(), run_id: getActiveRunId() }).catch(() => {});
      }
    }
  });
  refreshSidebar();
  loadModelPill();
  newChat();
}

boot();
