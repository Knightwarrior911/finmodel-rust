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
  refreshSidebar();
  loadModelPill();
  newChat();
}

boot();
