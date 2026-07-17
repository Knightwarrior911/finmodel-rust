// settings.mjs — settings modal: API key, model, EDGAR contact, output dir,
// Roam MCP command, and theme selection.

import { $, call, TAURI, escapeHtml, setTheme, themeChoice, activateDialog } from "./core.mjs";

let onSaved = () => {};
let deactivateDialog = null;

// OpenAI-compatible providers (users bring their own key). Base URLs verified
// against OMP's provider catalog. "custom" lets a user paste any endpoint.
const PROVIDERS = [
  { id: "openrouter", name: "OpenRouter", base: "https://openrouter.ai/api/v1" },
  { id: "openai", name: "OpenAI", base: "https://api.openai.com/v1" },
  { id: "xai", name: "xAI (Grok)", base: "https://api.x.ai/v1" },
  { id: "anthropic", name: "Anthropic (Claude)", base: "https://api.anthropic.com/v1" },
  { id: "gemini", name: "Google Gemini", base: "https://generativelanguage.googleapis.com/v1beta/openai" },
  { id: "deepseek", name: "DeepSeek", base: "https://api.deepseek.com/v1" },
  { id: "groq", name: "Groq", base: "https://api.groq.com/openai/v1" },
  { id: "mistral", name: "Mistral", base: "https://api.mistral.ai/v1" },
  { id: "together", name: "Together", base: "https://api.together.xyz/v1" },
  { id: "fireworks", name: "Fireworks", base: "https://api.fireworks.ai/inference/v1" },
  { id: "cerebras", name: "Cerebras", base: "https://api.cerebras.ai/v1" },
  { id: "moonshot", name: "Moonshot (Kimi)", base: "https://api.moonshot.ai/v1" },
  { id: "custom", name: "Custom (OpenAI-compatible)", base: "" },
];

function populateProviders() {
  const sel = $("providerSelect");
  if (!sel || sel.options.length) return;
  sel.innerHTML = PROVIDERS.map((p) => `<option value="${p.id}">${escapeHtml(p.name)}</option>`).join("");
}

function setProviderFromBase(base) {
  const b = (base || "").replace(/\/+$/, "");
  const match = PROVIDERS.find((p) => p.base && p.base === b);
  $("providerSelect").value = match ? match.id : b ? "custom" : "openrouter";
  $("baseUrl").value = b || "https://openrouter.ai/api/v1";
}

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
      ? "Your key is saved. Leave blank to keep it."
      : "No key yet — you're in demo mode with sample companies.";
    const sel = $("modelSelect");
    if (s.model) sel.innerHTML = `<option value="${escapeHtml(s.model)}">${escapeHtml(s.model)}</option>`;
    populateProviders();
    setProviderFromBase(s.base_url);
    $("edgarContact").value = s.edgar_contact || "";
    $("outDir").value = s.out_dir || "";
    $("mcpCommand").value = s.mcp_command || "";
    if ($("appVersion") && s.version) $("appVersion").textContent = `v${s.version}`;
    renderCaps(s.model_capability);
  } catch (_) {
    /* offline / first launch */
  }
  $("themeSelect").value = themeChoice();
  $("settingsModal").hidden = false;
  const card = $("settingsModal").querySelector(".modal-card");
  deactivateDialog = activateDialog(card, {
    initialFocus: "#apiKey",
    onEscape: closeSettings,
  });
}

function renderCaps(cap) {
  const el = $("modelCaps");
  if (!el) return;
  if (!cap || !cap.model_id) {
    el.textContent = "Capabilities untested — app routing + plain JSON until you run Test model.";
    return;
  }
  const tools = cap.native_tools ? "tools ✓" : "tools ✗";
  const json = cap.strict_json ? "strict JSON ✓" : "strict JSON ✗";
  const when = cap.tested_at ? ` · tested ${cap.tested_at}` : "";
  el.textContent = `${cap.model_id}: ${tools}, ${json}${when}`;
}

function closeSettings() {
  clearStatus();
  $("settingsModal").hidden = true;
  if (deactivateDialog) {
    deactivateDialog();
    deactivateDialog = null;
  }
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

  $("providerSelect").addEventListener("change", (e) => {
    const p = PROVIDERS.find((x) => x.id === e.target.value);
    if (!p) return;
    if (p.id === "custom") {
      $("baseUrl").value = "";
      $("baseUrl").focus();
    } else {
      $("baseUrl").value = p.base;
    }
  });

  $("saveSettings").addEventListener("click", async () => {
    const api_key = $("apiKey").value;
    const model = $("modelSelect").value || "";
    const cmd = ($("mcpCommand").value || "").trim();
    try {
      await call("save_settings", {
        api_key,
        model,
        base_url: $("baseUrl").value,
        edgar_contact: $("edgarContact").value,
        out_dir: $("outDir").value,
        mcp_command: cmd.split(/\s+/)[0] || "",
        mcp_args: cmd.split(/\s+/).slice(1),
      });
      $("apiKey").value = "";
      // Auto-detect what the model can do so the home screen is accurate
      // without the user running a manual check. Best-effort: a failed probe
      // (e.g. no key) just leaves capability unknown.
      if (model) {
        setStatus("Checking what your model can do…", "info");
        try {
          await call("test_model", { model_id: model });
        } catch {
          /* leave capability unknown */
        }
      }
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
      $("keyStatus").textContent = "No key yet — you're in demo mode with sample companies.";
      setStatus("Key removed — back to demo mode.", "info");
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
      await call("save_settings", { api_key: $("apiKey").value, model: "", base_url: $("baseUrl").value });
      $("apiKey").value = "";
      const models = await call("list_models");
      const sel = $("modelSelect");
      sel.innerHTML = models
        .map((m) => {
          const badges = [
            m.native_tools ? "tools" : null,
            m.strict_json ? "json" : null,
          ]
            .filter(Boolean)
            .join(",");
          const label = badges ? `${m.id} [${badges}]` : m.id;
          return `<option value="${escapeHtml(m.id)}">${escapeHtml(label)}</option>`;
        })
        .join("");
    } catch (e) {
      setStatus(`Model list failed: ${e.message || e}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = "Refresh";
    }
  });

  $("testModel").addEventListener("click", async () => {
    const btn = $("testModel");
    btn.disabled = true;
    btn.textContent = "Testing…";
    try {
      // Persist any typed key/model first so the probe uses current values.
      await call("save_settings", {
        api_key: $("apiKey").value,
        model: $("modelSelect").value || "",
        base_url: $("baseUrl").value,
      });
      $("apiKey").value = "";
      const cap = await call("test_model", {
        model_id: $("modelSelect").value || null,
      });
      renderCaps(cap);
      setStatus(
        `Tested ${cap.model_id}: tools=${cap.native_tools}, strict JSON=${cap.strict_json}`,
        "ok",
      );
    } catch (e) {
      renderCaps(null);
      setStatus(`Test model failed: ${e.message || e}`, "error");
    } finally {
      btn.disabled = false;
      btn.textContent = "Test model";
    }
  });
}
