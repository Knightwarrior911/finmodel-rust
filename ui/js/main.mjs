// main.mjs — boot: theme → reader → chat → sidebar → settings → update.

import { initTheme, call } from "./core.mjs";
import { initReader } from "./reader.mjs";
import { initSidebar, refresh as refreshSidebar, setActive } from "./sidebar.mjs";
import { initChat, loadConversation, newChat, getCurrentId, setModelPill } from "./chat.mjs";
import { initSettings } from "./settings.mjs";
import { initUpdate } from "./update.mjs";

async function loadModelPill() {
  try {
    const s = await call("load_settings");
    setModelPill(s.model);
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
        call("chat_cancel", { conversation_id: getCurrentId() }).catch(() => {});
      }
    }
  });
  refreshSidebar();
  loadModelPill();
  newChat();
}

boot();
