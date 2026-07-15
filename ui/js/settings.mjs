// settings.mjs — settings modal: API key, model, EDGAR contact, output dir,
// Roam MCP command, and theme selection.

import { $, call, TAURI, escapeHtml, setTheme, themeChoice } from "./core.mjs";

let onSaved = () => {};

function setStatus(msg, kind = "info") {
  const el = $("settingsStatus");
  if (!el) return;
  el.hidden = false;
  el.textContent = msg;
  el.className = `status ${kind}`;
}
function clearStatus() {
  const el = $("settingsStatus");
  if (el) el.hidden = true;
}

export async function openSettings() {
  clearStatus();
  try {
    const s = await call("load_settings");
    $("keyStatus").textContent = s.has_key
      ? "A key is saved. Leave blank to keep it."
      : "No key set — offline demo tickers only.";
    const sel = $("modelSelect");
    if (s.model) sel.innerHTML = `<option value="${escapeHtml(s.model)}">${escapeHtml(s.model)}</option>`;
    $("edgarContact").value = s.edgar_contact || "";
    $("outDir").value = s.out_dir || "";
    $("mcpCommand").value = s.mcp_command || "";
    if ($("appVersion") && s.version) $("appVersion").textContent = `v${s.version}`;
  } catch (_) {
    /* offline / first launch */
  }
  $("themeSelect").value = themeChoice();
  $("settingsModal").hidden = false;
  $("apiKey").focus();
}

function closeSettings() {
  clearStatus();
  $("settingsModal").hidden = true;
}

export function initSettings(opts = {}) {
  onSaved = opts.onSaved || (() => {});
  const modal = $("settingsModal");

  $("settingsBtn").addEventListener("click", openSettings);
  document.addEventListener("open-settings", openSettings);
  $("settingsClose").addEventListener("click", closeSettings);
  modal.addEventListener("click", (e) => {
    if (e.target && e.target.dataset && e.target.dataset.close) closeSettings();
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !modal.hidden) closeSettings();
  });

  $("themeSelect").addEventListener("change", (e) => {
    setTheme(e.target.value);
    document.dispatchEvent(new CustomEvent("theme-changed"));
  });

  $("saveSettings").addEventListener("click", async () => {
    const api_key = $("apiKey").value;
    const model = $("modelSelect").value || "";
    const cmd = ($("mcpCommand").value || "").trim();
    try {
      await call("save_settings", {
        api_key,
        model,
        edgar_contact: $("edgarContact").value,
        out_dir: $("outDir").value,
        mcp_command: cmd.split(/\s+/)[0] || "",
        mcp_args: cmd.split(/\s+/).slice(1),
      });
      $("apiKey").value = "";
      closeSettings();
      onSaved();
    } catch (e) {
      setStatus(`Save failed: ${e.message || e}`, "error");
    }
  });

  $("removeKey").addEventListener("click", async () => {
    try {
      await call("clear_api_key");
      $("apiKey").value = "";
      $("keyStatus").textContent = "No key set — offline demo tickers only.";
      setStatus("Key removed — offline demo mode.", "info");
      onSaved();
    } catch (e) {
      setStatus(`Remove failed: ${e.message || e}`, "error");
    }
  });

  $("chooseOutDir").addEventListener("click", async () => {
    const dlg = TAURI && TAURI.dialog;
    if (!dlg) return;
    try {
      const dir = await dlg.open({ directory: true });
      if (dir) $("outDir").value = dir;
    } catch (_) {
      /* cancelled */
    }
  });

  $("testMcp").addEventListener("click", async () => {
    const el = $("mcpStatus");
    const cmd = ($("mcpCommand").value || "").trim();
    if (!cmd) {
      el.textContent = "Enter the Roam MCP command first.";
      return;
    }
    el.textContent = "Testing…";
    try {
      const parts = cmd.split(/\s+/);
      const res = await call("test_mcp", { command: parts[0], args: parts.slice(1) });
      el.textContent = `Connected — ${res.tool_count} tools available.`;
    } catch (e) {
      el.textContent = `Connection failed: ${e.message || e}`;
    }
  });

  $("refreshModels").addEventListener("click", async () => {
    const btn = $("refreshModels");
    btn.disabled = true;
    btn.textContent = "Loading…";
    try {
      await call("save_settings", { api_key: $("apiKey").value, model: "" });
      $("apiKey").value = "";
      const models = await call("list_models");
      const sel = $("modelSelect");
      sel.innerHTML = models
        .map((m) => `<option value="${escapeHtml(m.id)}">${escapeHtml(m.id)}</option>`)
        .join("");
    } catch (e) {
      setStatus(`Model list failed: ${e.message || e}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = "Refresh";
    }
  });
}
