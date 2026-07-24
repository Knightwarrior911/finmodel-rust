// settings.mjs — settings modal: API key, model, EDGAR contact, output dir,
// Roam MCP command, and theme selection.

import {
  $,
  call,
  TAURI,
  escapeHtml,
  setTheme,
  themeChoice,
  activateDialog,
} from "./core.mjs";

let onSaved = () => {};
let deactivateDialog = null;

// OpenAI-compatible providers (users bring their own key). Base URLs verified
// against OMP's provider catalog. "custom" lets a user paste any endpoint.
const PROVIDERS = [
  {
    id: "openrouter",
    name: "OpenRouter",
    base: "https://openrouter.ai/api/v1",
  },
  { id: "openai", name: "OpenAI", base: "https://api.openai.com/v1" },
  { id: "xai", name: "xAI (Grok)", base: "https://api.x.ai/v1" },
  {
    id: "anthropic",
    name: "Anthropic (Claude)",
    base: "https://api.anthropic.com/v1",
  },
  {
    id: "gemini",
    name: "Google Gemini",
    base: "https://generativelanguage.googleapis.com/v1beta/openai",
  },
  { id: "deepseek", name: "DeepSeek", base: "https://api.deepseek.com/v1" },
  { id: "groq", name: "Groq", base: "https://api.groq.com/openai/v1" },
  { id: "mistral", name: "Mistral", base: "https://api.mistral.ai/v1" },
  { id: "together", name: "Together", base: "https://api.together.xyz/v1" },
  {
    id: "fireworks",
    name: "Fireworks",
    base: "https://api.fireworks.ai/inference/v1",
  },
  { id: "cerebras", name: "Cerebras", base: "https://api.cerebras.ai/v1" },
  {
    id: "moonshot",
    name: "Moonshot (Kimi)",
    base: "https://api.moonshot.ai/v1",
  },
  { id: "custom", name: "Custom (OpenAI-compatible)", base: "" },
];

// Personal-use subscription providers (OpenCode Go + Cursor via OMP gateway).
// Empty when FINMODEL_DISABLE_SUBSCRIPTION_PROVIDERS=1.
let SUBSCRIPTION_PROVIDERS = [];
// Preserve the identity of a saved gateway selection while its provider is
// temporarily offline. It remains visible only as the current selection; the
// live status response still controls which other subscriptions are offered.
let SAVED_SUBSCRIPTION_PROVIDER = null;

function subscriptionProviderFromSelection(base, model = "") {
  const normalized = String(base || "").replace(/\/+$/, "");
  const gateway = "http://127.0.0.1:4000/v1";
  if (normalized !== gateway && normalized !== "http://localhost:4000/v1") return null;
  const opencode = String(model).startsWith("opencode-go/");
  return {
    id: opencode ? "opencode-go" : "cursor",
    name: opencode ? "OpenCode Go" : "Cursor (via OMP gateway)",
    base: gateway,
  };
}

function activeProviders() {
  // Keep Custom last so subscription entries sit with the named providers.
  const base = PROVIDERS.filter((p) => p.id !== "custom");
  const custom = PROVIDERS.find((p) => p.id === "custom");
  const subscriptions = [...SUBSCRIPTION_PROVIDERS];
  if (
    SAVED_SUBSCRIPTION_PROVIDER &&
    !subscriptions.some((p) => p.id === SAVED_SUBSCRIPTION_PROVIDER.id)
  ) {
    subscriptions.push(SAVED_SUBSCRIPTION_PROVIDER);
  }
  const out = [...base, ...subscriptions];
  if (custom) out.push(custom);
  return out;
}

let cursorLoginPollTimer = null;
let opencodeLoginPollTimer = null;

function stopOpenCodeLoginPoll() {
  if (opencodeLoginPollTimer) {
    clearInterval(opencodeLoginPollTimer);
    opencodeLoginPollTimer = null;
  }
}

function startOpenCodeLoginPoll(onReady) {
  stopOpenCodeLoginPoll();
  let ticks = 0;
  opencodeLoginPollTimer = setInterval(async () => {
    ticks += 1;
    if (ticks > 150) {
      stopOpenCodeLoginPoll();
      return;
    }
    try {
      const st = await refreshSubscriptionProviders();
      populateProviders();
      populateProviderBaseList();
      if (st?.opencode?.chat_ready) {
        stopOpenCodeLoginPoll();
        if (onReady) await onReady();
      }
    } catch (_) {}
  }, 2000);
  opencodeLoginPollTimer.unref?.();
}


function normalizeModelEntries(raw) {
  let value = raw;
  if (typeof value === "string") {
    try { value = JSON.parse(value); } catch (_) { return []; }
  }
  const entries = Array.isArray(value) ? value : value?.models;
  if (!Array.isArray(entries)) return [];
  return entries
    .map((m) => (typeof m === "string" ? { id: m, name: m } : m))
    .filter((m) => m && m.id)
    .map((m) => ({ ...m, id: String(m.id) }));
}

function modelOptionLabel(m) {
  const name = String(m.name || "").trim();
  return name && name !== m.id ? name + " — " + m.id : m.id;
}

async function fetchProviderModels(providerId) {
  if (providerId === "cursor") {
    const res = await call("probe_cursor_models");
    return normalizeModelEntries(res);
  }
  const payload = providerId ? { provider_id: providerId } : {};
  return normalizeModelEntries(await call("list_models", payload));
}

async function refreshModelOptionsForProvider(providerId, preferredModel = "") {
  const sel = $("modelSelect");
  if (!sel) return [];
  const current = preferredModel || sel.value || "";
  try {
    const models = await fetchProviderModels(providerId);
    if (!models.length) {
      if (current) sel.innerHTML = "<option value=\"" + escapeHtml(current) + "\">" + escapeHtml(current) + "</option>";
      return models;
    }
    sel.innerHTML = models.map((m) => "<option value=\"" + escapeHtml(m.id) + "\">" + escapeHtml(modelOptionLabel(m)) + "</option>").join("");
    if (models.some((m) => m.id === current)) sel.value = current;
    return models;
  } catch (_) {
    if (current) sel.innerHTML = "<option value=\"" + escapeHtml(current) + "\">" + escapeHtml(current) + "</option>";
    return [];
  }
}

function stopCursorLoginPoll() {
  if (cursorLoginPollTimer) {
    clearInterval(cursorLoginPollTimer);
    cursorLoginPollTimer = null;
  }
}

async function refreshSubscriptionProviders() {
  const box = $("subscriptionProvidersBox");
  try {
    const st = await call("subscription_providers_status");
    // Only chat-ready providers enter the dropdown. Cursor uses the local
    // OMP auth-gateway base_url (never raw api2.cursor.sh).
    SUBSCRIPTION_PROVIDERS = st.enabled
      ? (st.providers || [])
          .filter((p) => p && p.id && p.chat_ready !== false)
          .map((p) => ({
            id: p.id,
            name: p.name,
            base: p.base,
          }))
      : [];
    if (box) {
      box.hidden = !st.enabled;
      const note = $("subscriptionProvidersNote");
      const cursorNote = $("cursorProbeStatus");
      const goNote = $("importOpencodeStatus");
      const c = st.cursor || {};
      const o = st.opencode || {};
      const cursorReusable = !!c.present && (!c.expired || !!c.refresh_present);
      if (note && st.enabled) {
        const parts = [];
        if (o.chat_ready) parts.push("OpenCode Go live gateway verified.");
        else if (o.credential_present) parts.push("OpenCode Go login stored; use it to verify the live gateway.");
        else parts.push("OpenCode Go needs Connect through OMP.");
        if (c.chat_ready) parts.push("Cursor live gateway verified.");
        else if (cursorReusable) parts.push("Cursor login stored; use it to refresh and verify the live gateway.");
        else if (c.present) parts.push("Cursor login expired and cannot refresh; connect again.");
        else parts.push("Cursor needs Connect through OMP browser login.");
        note.textContent = parts.join(" ");
      }
      if (goNote && st.enabled) {
        goNote.textContent = o.reason || goNote.textContent || "";
      }
      if (cursorNote) {
        if (!st.enabled) cursorNote.textContent = "";
        else if (c.chat_ready)
          cursorNote.textContent =
            "Cursor via OMP: live gateway verified. Prefer cursor/claude-4.6-sonnet-medium or cursor/default.";
        else
          cursorNote.textContent =
            c.reason || "Cursor live gateway not verified — connect or use an existing login.";
      }
      const btnConnectCur = $("connectCursorOmp");
      const btnUseCur = $("useCursorOmp");
      if (btnConnectCur) btnConnectCur.hidden = !st.enabled || cursorReusable;
      if (btnUseCur) btnUseCur.hidden = !st.enabled || !cursorReusable;
    }
    return st;
  } catch (_) {
    SUBSCRIPTION_PROVIDERS = [];
    if (box) box.hidden = true;
    return null;
  }
}

function startCursorLoginPoll(onReady) {
  stopCursorLoginPoll();
  let ticks = 0;
  cursorLoginPollTimer = setInterval(async () => {
    ticks += 1;
    if (ticks > 150) {
      stopCursorLoginPoll();
      return;
    }
    try {
      const st = await refreshSubscriptionProviders();
      populateProviders();
      populateProviderBaseList();
      if (st && st.cursor && (st.cursor.chat_ready || st.cursor.available)) {
        stopCursorLoginPoll();
        if (onReady) await onReady();
      }
    } catch (_) {}
  }, 2000);
}


async function applyCursorProvider() {
  const res = await call("use_cursor_omp");
  await refreshSubscriptionProviders();
  populateProviders();
  populateProviderBaseList();
  setProviderFromBase(res.base_url, res.model);
  if (res.model && $("modelSelect")) $("modelSelect").value = res.model;
  return res;
}

function populateProviders() {
  const sel = $("providerSelect");
  if (!sel) return;
  const current = sel.value;
  const providers = activeProviders();
  sel.innerHTML = providers
    .map((p) => `<option value="${p.id}">${escapeHtml(p.name)}</option>`)
    .join("");
  if (current && providers.some((p) => p.id === current)) sel.value = current;
}
/// Fill the shared datalists so every model/provider field offers picks as
/// you type (no more hand-typing model ids). Provider addresses are static;
/// the model list comes from the live catalog when a key is saved.
function populateProviderBaseList() {
  const dl = document.getElementById("providerBaseList");
  if (!dl) return;
  dl.innerHTML = activeProviders()
    .filter((p) => p.base)
    .map(
      (p) =>
        `<option value="${escapeHtml(p.base)}">${escapeHtml(p.name)}</option>`,
    )
    .join("");
}

async function populateModelCatalogList() {
  const dl = document.getElementById("modelCatalogList");
  if (!dl) return;
  try {
    const providerId = $("providerSelect")?.value || "";
    const raw = await call("list_models", providerId ? { provider_id: providerId } : {});
    const models = normalizeModelEntries(raw);
    dl.innerHTML = models
      .map((m) => {
        const badges = [
          m.native_tools ? "uses tools" : null,
          m.vision ? "sees images" : null,
        ].filter(Boolean).join(" · ");
        const label = badges ? modelOptionLabel(m) + " · " + badges : modelOptionLabel(m);
        return "<option value=\"" + escapeHtml(m.id) + "\">" + escapeHtml(label) + "</option>";
      })
      .join("");
  } catch (_) {
    /* no key yet / offline — fields still accept typing */
  }
}
function setProviderFromBase(base, model = "") {
  const b = (base || "").replace(/\/+$/, "");
  const savedSubscription = subscriptionProviderFromSelection(b, model);
  $("baseUrl").value = savedSubscription?.base || b || "https://openrouter.ai/api/v1";
  if (savedSubscription) {
    SAVED_SUBSCRIPTION_PROVIDER = savedSubscription;
    $("providerSelect").value = savedSubscription.id;
    return;
  }
  SAVED_SUBSCRIPTION_PROVIDER = null;
  const match = activeProviders().find((p) => p.base && p.base === b);
  $("providerSelect").value = match ? match.id : b ? "custom" : "openrouter";
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

// ── Model-role profiles (Task 1.5) ───────────────────────────────────────────
// The worker runs delegated child tasks; the verifier is an optional extra check.
// A profile is only sent when both provider base + model are present; blank roles
// clear back to orchestrator-only. `credential_ref` names an OS-credential-store
// account (the secret itself is never in the frontend).

function readRoleProfile(prefix) {
  const base = ($(`${prefix}ProviderBase`).value || "").trim();
  const model = ($(`${prefix}Model`).value || "").trim();
  const cred = ($(`${prefix}CredentialRef`).value || "").trim();
  if (!base || !model) return null;
  return { provider_base: base, model, credential_ref: cred };
}

function fillRoleProfile(prefix, p) {
  $(`${prefix}ProviderBase`).value = p ? p.provider_base || "" : "";
  $(`${prefix}Model`).value = p ? p.model || "" : "";
  $(`${prefix}CredentialRef`).value = p ? p.credential_ref || "" : "";
}

/// Assemble the `model_profiles` payload from the current role inputs.
export function readModelProfiles() {
  return {
    worker: readRoleProfile("worker"),
    verifier: readRoleProfile("verifier"),
    fallbacks: [],
  };
}

// Settings sections: roving tablist mirroring the evidence dock (one
// vocabulary — same classes, same keyboard map: ←/→/Home/End).
const SETTINGS_TABS = ["general", "connections", "memory", "skills", "agents", "scheduled"];

export function selectSettingsTab(tab) {
  if (!SETTINGS_TABS.includes(tab)) return;
  for (const t of SETTINGS_TABS) {
    const btn = document.getElementById(`settingsTab-${t}`);
    const panel = document.getElementById(`settingsPanel-${t}`);
    if (!btn || !panel) continue;
    const sel = t === tab;
    btn.setAttribute("aria-selected", String(sel));
    btn.tabIndex = sel ? 0 : -1;
    panel.hidden = !sel;
  }
}

export async function openSettings() {
  selectSettingsTab("general");
  clearStatus();
  try {
    const s = await call("load_settings");
    SAVED_SUBSCRIPTION_PROVIDER = subscriptionProviderFromSelection(s.base_url, s.model);
    const subscriptionGateway = SAVED_SUBSCRIPTION_PROVIDER !== null;
    $("keyStatus").textContent = subscriptionGateway
      ? "Subscription credential stays in OMP."
      : s.has_key
        ? "Your key is saved. Leave blank to keep it."
        : "No key yet — you're in demo mode with sample companies.";
    const sel = $("modelSelect");
    if (s.model)
      sel.innerHTML = `<option value="${escapeHtml(s.model)}">${escapeHtml(s.model)}</option>`;
    await refreshSubscriptionProviders();
    populateProviders();
    populateProviderBaseList();
    setProviderFromBase(s.base_url, s.model);
    await refreshModelOptionsForProvider($("providerSelect")?.value || "", s.model);
    if (s.has_key) populateModelCatalogList(); // fire-and-forget pick list
    $("edgarContact").value = s.edgar_contact || "";
    $("outDir").value = s.out_dir || "";
    $("mcpCommand").value = s.mcp_command || "";
    $("synthesisModel").value = s.synthesis_model || "";
  $("advisorModel").value = s.advisor_model || "";
    $("autoRouteVision").checked = s.auto_route_vision !== false;
    $("routePriceCap").value =
      typeof s.route_price_cap_usd === "number" ? s.route_price_cap_usd : "";
    $("conversationBudget").value =
      s.conversation_budget_usd > 0 ? s.conversation_budget_usd : "";
    $("globalInstructions").value = s.global_instructions || "";
    $("edinetKey").placeholder = s.has_edinet_key
      ? "A key is saved — leave blank to keep it"
      : "EDINET API key";
    if ($("appVersion") && s.version)
      $("appVersion").textContent = `v${s.version}`;
    renderCaps(s.model_capability);
    const mp = s.model_profiles || {};
    fillRoleProfile("worker", mp.worker);
    fillRoleProfile("verifier", mp.verifier);
    loadMemoryList();
    loadSkillsList();
    loadAgentsList();
    loadSchedulesList();
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
    el.textContent =
      "Not checked yet — click Test model and I'll find out what this model can do.";
    return;
  }
  const tools = cap.native_tools
    ? "can use tools ✓"
    : cap.model_id.startsWith("cursor/")
      ? "direct/synthesis-only (no finmodel tools)"
      : "can't use tools ✗";
  const json = cap.strict_json ? "reliable tables ✓" : "loose tables";
  const when = cap.tested_at ? ` · checked ${cap.tested_at.slice(0, 10)}` : "";
  el.textContent = `${cap.model_id} — ${tools}, ${json}${when}`;
}

async function loadMemoryList() {
  const el = $("memoryList");
  if (!el) return;
  el.innerHTML = '<span class="field-hint">Loading…</span>';
  let mems = [];
  try {
    mems = await call("memory_list");
  } catch {
    el.innerHTML = '<span class="field-hint">Could not load memories.</span>';
    return;
  }
  el.innerHTML = "";
  if (!mems.length) {
    el.innerHTML = '<span class="field-hint">No saved memories yet.</span>';
    return;
  }
  for (const m of mems) {
    const row = document.createElement("div");
    row.className = "memory-row";
    row.dataset.content = m.content;
    const txt = document.createElement("span");
    txt.className = "memory-row-text";
    txt.textContent = m.content;
    if (m.pinned) {
      const badge = document.createElement("span");
      badge.className = "memory-pin-badge";
      badge.innerHTML =
        ' <svg width="11" height="11" viewBox="0 0 24 24" fill="currentColor" aria-hidden="true"><path d="M16 3l5 5-6.5 2.2L12 14l-2-2-5.5 7.5L3 19l7.5-5.5-2-2 3.8-2.5z"/></svg>';
      badge.setAttribute("aria-label", "pinned");
      badge.setAttribute("role", "img");
      txt.appendChild(badge);
    }
    // Pin/unpin: protect a good memory from automatic forgetting (Task 7.2).
    const pin = document.createElement("button");
    pin.type = "button";
    pin.className = "btn-ghost";
    pin.textContent = m.pinned ? "Unpin" : "Pin";
    pin.dataset.pin = String(m.id);
    pin.addEventListener("click", async () => {
      try {
        await call("memory_pin", { id: m.id, pinned: !m.pinned });
        loadMemoryList();
      } catch {
        /* leave row; user can retry */
      }
    });
    // Edit: inline-correct a memory's text (Task 7.2). Swaps the row for an
    // input + Save; reloads on success.
    const edit = document.createElement("button");
    edit.type = "button";
    edit.className = "btn-ghost";
    edit.textContent = "Edit";
    edit.dataset.edit = String(m.id);
    edit.addEventListener("click", () => {
      const input = document.createElement("input");
      input.type = "text";
      input.className = "memory-edit-input";
      input.value = m.content;
      const save = document.createElement("button");
      save.type = "button";
      save.className = "btn-ghost";
      save.textContent = "Save";
      save.dataset.editSave = String(m.id);
      save.addEventListener("click", async () => {
        const v = input.value.trim();
        if (!v) return;
        try {
          await call("memory_edit", { id: m.id, value: v });
          loadMemoryList();
        } catch {
          /* leave editor open; user can retry */
        }
      });
      row.replaceChildren(input, save);
      input.focus();
    });
    const del = document.createElement("button");
    del.type = "button";
    del.className = "btn-ghost";
    del.textContent = "Delete";
    del.addEventListener("click", async () => {
      try {
        await call("memory_delete", { id: m.id });
        row.remove();
        if (!el.querySelector(".memory-row")) {
          el.innerHTML =
            '<span class="field-hint">No saved memories yet.</span>';
        }
      } catch {
        /* leave row; user can retry */
      }
    });
    row.appendChild(txt);
    row.appendChild(edit);
    row.appendChild(pin);
    row.appendChild(del);
    el.appendChild(row);
  }
}

async function loadAgentsList() {
  const el = $("agentsList");
  if (!el) return;
  el.innerHTML = '<span class="field-hint">Loading…</span>';
  try {
    const agents = await call("agents_list");
    if (!agents.length) {
      el.innerHTML =
        '<span class="field-hint">No agents yet. Describe one below - a name, what it specializes in, and its working doctrine.</span>';
      return;
    }
    el.innerHTML = "";
    for (const a of agents) {
      const row = document.createElement("div");
      row.className = "memory-row";
      const txt = document.createElement("span");
      txt.className = "memory-row-text";
      const skills = a.skills && a.skills.length
        ? ` <span class="skill-uses">· skills: ${escapeHtml(a.skills.join(", "))}</span>`
        : "";
      txt.innerHTML = `<b>${escapeHtml(a.name)}</b>${skills} — ${escapeHtml(a.description)}`;
      row.appendChild(txt);
      const edit = document.createElement("button");
      edit.type = "button";
      edit.className = "btn-ghost";
      edit.textContent = "Edit";
      edit.addEventListener("click", async () => {
        const existing = row.nextElementSibling;
        if (existing && existing.classList.contains("skill-editor")) {
          existing.remove();
          return;
        }
        let content = "";
        try {
          content = await call("agents_get", { name: a.name });
        } catch (_) {
          return;
        }
        const box = document.createElement("div");
        box.className = "skill-editor";
        const ta = document.createElement("textarea");
        ta.rows = 14;
        ta.value = content;
        ta.setAttribute("aria-label", `Edit agent ${a.name}`);
        const save = document.createElement("button");
        save.type = "button";
        save.className = "btn-ghost";
        save.textContent = "Save";
        const cancel = document.createElement("button");
        cancel.type = "button";
        cancel.className = "btn-ghost";
        cancel.textContent = "Cancel";
        const st = document.createElement("span");
        st.className = "field-hint";
        cancel.addEventListener("click", () => box.remove());
        save.addEventListener("click", async () => {
          const v = ta.value;
          const m = v.match(/^\s*name:\s*(.+)$/m);
          const newName = m ? m[1].trim().replace(/^["']|["']$/g, "") : "";
          if (!newName) {
            st.textContent = "Frontmatter needs a `name:` line.";
            return;
          }
          try {
            await call("agents_save", { name: newName, content: v });
            if (newName !== a.name) {
              await call("agents_delete", { name: a.name });
            }
            loadAgentsList();
          } catch (e) {
            st.textContent = (e && e.message) || "Save failed.";
          }
        });
        const btns = document.createElement("div");
        btns.className = "skill-editor-btns";
        btns.appendChild(save);
        btns.appendChild(cancel);
        btns.appendChild(st);
        box.appendChild(ta);
        box.appendChild(btns);
        row.after(box);
        ta.focus();
      });
      row.appendChild(edit);
      const del = document.createElement("button");
      del.type = "button";
      del.className = "btn-ghost";
      del.textContent = "Delete";
      del.addEventListener("click", async () => {
        try {
          await call("agents_delete", { name: a.name });
          loadAgentsList();
        } catch (_) {}
      });
      row.appendChild(del);
      el.appendChild(row);
    }
  } catch (_) {
    el.innerHTML = '<span class="field-hint">Couldn\u2019t load agents.</span>';
  }
}

async function loadSkillsList() {
  const el = $("skillsList");
  if (!el) return;
  el.innerHTML = '<span class="field-hint">Loading…</span>';
  try {
    const skills = await call("skills_list");
    if (!skills.length) {
      el.innerHTML =
        '<span class="field-hint">No skills yet. Add one below, or finish a multi-step task in chat and choose "Save as skill".</span>';
      return;
    }
    el.innerHTML = "";
    for (const s of skills) {
      const row = document.createElement("div");
      row.className = "memory-row";
      const txt = document.createElement("span");
      txt.className = "memory-row-text";
      const stateLabel =
        s.state && s.state !== "active"
          ? ` <span class="skill-state">(${escapeHtml(s.state)})</span>`
          : "";
      // Surface how often the analyst actually used this skill (lifecycle).
      const uses =
        s.use_count > 0
          ? ` <span class="skill-uses">· used ${Number(s.use_count)}×</span>`
          : "";
      txt.innerHTML = `<b>${escapeHtml(s.name)}</b>${stateLabel}${uses} — ${escapeHtml(s.description)}`;
      row.appendChild(txt);
      // View/edit the full SKILL.md inline (skills_get → skills_save). Saving
      // under a renamed frontmatter `name` moves the file (old name deleted).
      const edit = document.createElement("button");
      edit.type = "button";
      edit.className = "btn-ghost";
      edit.textContent = "Edit";
      edit.dataset.skillEdit = s.name;
      edit.addEventListener("click", async () => {
        const existing = row.nextElementSibling;
        if (existing && existing.classList.contains("skill-editor")) {
          existing.remove(); // toggle closed
          return;
        }
        let content = "";
        try {
          content = await call("skills_get", { name: s.name });
        } catch (_) {
          return;
        }
        const box = document.createElement("div");
        box.className = "skill-editor";
        const ta = document.createElement("textarea");
        ta.rows = 14;
        ta.value = content;
        ta.setAttribute("aria-label", `Edit skill ${s.name}`);
        const save = document.createElement("button");
        save.type = "button";
        save.className = "btn-ghost";
        save.textContent = "Save";
        save.dataset.skillSave = s.name;
        const cancel = document.createElement("button");
        cancel.type = "button";
        cancel.className = "btn-ghost";
        cancel.textContent = "Cancel";
        const st = document.createElement("span");
        st.className = "field-hint";
        cancel.addEventListener("click", () => box.remove());
        save.addEventListener("click", async () => {
          const v = ta.value;
          const m = v.match(/^\s*name:\s*(.+)$/m);
          const newName = m ? m[1].trim().replace(/^["']|["']$/g, "") : "";
          if (!newName) {
            st.textContent = "Frontmatter needs a `name:` line.";
            return;
          }
          try {
            await call("skills_save", { name: newName, content: v });
            // Rename semantics: a changed frontmatter name moves the skill.
            if (newName !== s.name) {
              await call("skills_delete", { name: s.name });
            }
            loadSkillsList();
          } catch (e) {
            st.textContent = (e && e.message) || "Save failed.";
          }
        });
        const btns = document.createElement("div");
        btns.className = "skill-editor-btns";
        btns.appendChild(save);
        btns.appendChild(cancel);
        btns.appendChild(st);
        box.appendChild(ta);
        box.appendChild(btns);
        row.after(box);
        ta.focus();
      });
      row.appendChild(edit);
      // Restore a stale/archived skill back into default context (Task 7.2/7.3).
      if (s.state === "stale" || s.state === "archived") {
        const restore = document.createElement("button");
        restore.type = "button";
        restore.className = "btn-ghost";
        restore.textContent = "Restore";
        restore.dataset.restore = s.name;
        restore.addEventListener("click", async () => {
          try {
            await call("skill_restore", { name: s.name });
            loadSkillsList();
          } catch (_) {
            /* ignore */
          }
        });
        row.appendChild(restore);
      }
      const del = document.createElement("button");
      del.type = "button";
      del.className = "btn-ghost";
      del.textContent = "Delete";
      del.addEventListener("click", async () => {
        try {
          await call("skills_delete", { name: s.name });
          loadSkillsList();
        } catch (_) {
          /* ignore */
        }
      });
      row.appendChild(del);
      el.appendChild(row);
    }
  } catch (_) {
    el.innerHTML = '<span class="field-hint">Could not load skills.</span>';
  }
}

/// Open Settings with the New-skill editor pre-filled (self-evolution draft).
/// Scheduled follow-ups: list + cancel (approval-created in chat; the tick
/// runs them). Quiet rows: what, when, how often, and a way out.
export async function loadSchedulesList() {
  const list = $("schedulesList");
  if (!list) return;
  try {
    const rows = await call("schedules_list");
    const items = Array.isArray(rows) ? rows : [];
    if (items.length === 0) {
      list.innerHTML =
        '<li class="card-note">Nothing scheduled. Ask me to "re-run this after earnings" or "remind me next week" in any chat.</li>';
      return;
    }
    list.innerHTML = items
      .map((r) => {
        let prompt = "";
        try {
          prompt = JSON.parse(r.scope_json || "{}").prompt || "";
        } catch {
          /* opaque scope */
        }
        const when = (r.next_due || "").slice(0, 10);
        const rec =
          r.recurrence === "daily"
            ? " · every day"
            : r.recurrence === "weekly"
              ? " · every week"
              : "";
        const status = r.status === "pending" ? "" : ` · ${escapeHtml(r.status)}`;
        return `<li class="schedule-row" data-id="${escapeHtml(r.id)}">
          <span class="schedule-row-main">
            <span class="schedule-row-prompt">${escapeHtml(prompt || "(scheduled task)")}</span>
            <span class="schedule-row-meta num">due ${escapeHtml(when)}${rec}${status}</span>
          </span>
          <button type="button" class="btn-ghost schedule-cancel" data-id="${escapeHtml(r.id)}">Cancel</button>
        </li>`;
      })
      .join("");
    list.querySelectorAll(".schedule-cancel").forEach((btn) => {
      btn.addEventListener("click", async () => {
        try {
          await call("schedule_cancel", { id: btn.dataset.id });
          loadSchedulesList();
        } catch {
          /* row stays; next open retries */
        }
      });
    });
  } catch {
    list.innerHTML = '<li class="card-note">Couldn\'t load schedules just now.</li>';
  }
}

export function openSettingsWithSkillDraft(draft) {
  openSettings();
  selectSettingsTab("skills");
  const ta = $("newSkillContent");
  if (ta) {
    ta.value = draft || "";
    const d = ta.closest("details");
    if (d) d.open = true;
    ta.focus();
  }
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
  // Filter saved memories client-side by content substring (Task 7.2).
  $("memoryFilter")?.addEventListener("input", (e) => {
    const q = (e.target.value || "").trim().toLowerCase();
    for (const row of document.querySelectorAll("#memoryList .memory-row")) {
      const c = (row.dataset.content || "").toLowerCase();
      row.hidden = !!q && !c.includes(q);
    }
  });
  $("settingsClose").addEventListener("click", closeSettings);
  // Section tabs: click selects; ←/→/Home/End rove (mirrors the dock tablist).
  const tablist = modal.querySelector(".settings-tabs");
  tablist?.addEventListener("click", (e) => {
    const tab = e.target?.dataset?.settingsTab;
    if (tab) selectSettingsTab(tab);
  });
  tablist?.addEventListener("keydown", (e) => {
    const cur = SETTINGS_TABS.indexOf(
      document.activeElement?.dataset?.settingsTab,
    );
    if (cur < 0) return;
    let next = -1;
    if (e.key === "ArrowRight" || e.key === "ArrowDown")
      next = (cur + 1) % SETTINGS_TABS.length;
    else if (e.key === "ArrowLeft" || e.key === "ArrowUp")
      next = (cur + SETTINGS_TABS.length - 1) % SETTINGS_TABS.length;
    else if (e.key === "Home") next = 0;
    else if (e.key === "End") next = SETTINGS_TABS.length - 1;
    if (next < 0) return;
    e.preventDefault();
    const t = SETTINGS_TABS[next];
    selectSettingsTab(t);
    document.getElementById(`settingsTab-${t}`)?.focus();
  });
  $("skillSaveBtn")?.addEventListener("click", async () => {
    const content = $("newSkillContent").value;
    const m = content.match(/^\s*name:\s*(.+)$/m);
    const name = m ? m[1].trim().replace(/^["']|["']$/g, "") : "";
    const st = $("skillStatus");
    if (!name) {
      if (st) st.textContent = "Give the skill a name first — a name: line at the very top.";
      return;
    }
    try {
      await call("skills_save", { name, content });
      $("newSkillContent").value = "";
      if (st) st.textContent = "Saved.";
      loadSkillsList();
    } catch (e) {
      if (st) st.textContent = (e && e.message) || "Save failed.";
    }
  });
  $("agentSaveBtn")?.addEventListener("click", async () => {
    const content = $("newAgentContent").value;
    const m = content.match(/^\s*name:\s*(.+)$/m);
    const name = m ? m[1].trim().replace(/^["']|["']$/g, "") : "";
    const st = $("agentStatus");
    if (!name) {
      if (st) st.textContent = "Give the agent a name first — a name: line at the very top.";
      return;
    }
    try {
      await call("agents_save", { name, content });
      $("newAgentContent").value = "";
      if (st) st.textContent = "Saved.";
      loadAgentsList();
    } catch (e) {
      if (st) st.textContent = (e && e.message) || "Save failed.";
    }
  });
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

  $("providerSelect").addEventListener("change", async (e) => {
    const p = activeProviders().find((x) => x.id === e.target.value);
    if (!p) return;
    if (p.id === "custom") {
      $("baseUrl").value = "";
      $("baseUrl").focus();
      return;
    }
    if (p.id === "opencode-go") {
      try {
        setStatus("Connecting OpenCode Go…", "info");
        const res = await call("connect_opencode_go");
        await refreshSubscriptionProviders();
        populateProviders();
        populateProviderBaseList();
        if (!res.needs_auth && res.base_url) {
          setProviderFromBase(res.base_url, res.model || "");
          await refreshModelOptionsForProvider("opencode-go", res.model || "");
        } else if (!res.needs_auth) {
          $("baseUrl").value = p.base;
        }
        if (res.needs_auth) {
          startOpenCodeLoginPoll(() => connectGo?.click());
          setStatus(
            res.guidance || "Finish OpenCode Go login in the OMP terminal.",
            "info",
          );
        } else {
          stopOpenCodeLoginPoll();
          $("keyStatus").textContent = "Subscription credential stays in OMP.";
          setStatus("OpenCode Go connected through OMP.", "info");
          onSaved();
        }
      } catch (err) {
        setStatus("OpenCode Go setup failed: " + (err.message || err), "error");
        $("baseUrl").value = p.base;
      }
      return;
    }
    if (p.id === "cursor") {
      try {
        setStatus("Connecting Cursor via OMP…", "info");
        const res = await call("connect_cursor_omp");
        if (res.chat_ready && res.base_url) {
          await refreshSubscriptionProviders();
          populateProviders();
          populateProviderBaseList();
          setProviderFromBase(res.base_url, res.model);
          if (res.model && $("modelSelect")) $("modelSelect").value = res.model;
          await refreshModelOptionsForProvider("cursor", res.model || "");
          setStatus("Cursor ready via OMP gateway — model " + (res.model || "cursor/claude-4.6-sonnet-medium") + ".", "info");
          onSaved();
        } else {
          $("baseUrl").value = p.base;
          setStatus(res.guidance || "Complete Cursor login in the omp window.", "info");
          startCursorLoginPoll(async () => {
            const wired = await applyCursorProvider();
            await refreshModelOptionsForProvider("cursor", wired.model || "");
            setStatus("Cursor ready via OMP gateway — model " + (wired.model || "cursor/claude-4.6-sonnet-medium") + ".", "info");
            onSaved();
          });
        }
      } catch (err) {
        setStatus("Cursor setup failed: " + (err.message || err), "error");
        $("baseUrl").value = p.base;
      }
      return;
    }

    $("baseUrl").value = p.base;
    await refreshModelOptionsForProvider(p.id);
  });

  $("saveSettings").addEventListener("click", async () => {
    const api_key = $("apiKey").value;
    const model = $("modelSelect").value || "";
    const cmd = ($("mcpCommand").value || "").trim();
    // Money fields: blank = keep what's saved; anything typed must be a
    // number ≥ 0 (the backend double-checks and refuses junk).
    const money = (id) => {
      const raw = ($(id).value || "").trim();
      if (raw === "") return undefined;
      const n = Number(raw);
      return Number.isFinite(n) && n >= 0 ? n : NaN;
    };
    const cap = money("routePriceCap");
    const budget = money("conversationBudget");
    if (Number.isNaN(cap) || Number.isNaN(budget)) {
      setStatus(
        "The dollar limits need a plain number like 5 or 2.50 (or 0 to turn them off).",
        "error",
      );
      return;
    }
    try {
      await call("save_settings", {
        api_key,
        model,
        base_url: $("baseUrl").value,
        edgar_contact: $("edgarContact").value,
        out_dir: $("outDir").value,
        mcp_command: cmd.split(/\s+/)[0] || "",
        mcp_args: cmd.split(/\s+/).slice(1),
        synthesis_model: $("synthesisModel").value,
        advisor_model: $("advisorModel").value,
        model_profiles: readModelProfiles(),
        auto_route_vision: $("autoRouteVision").checked,
        ...(cap !== undefined ? { route_price_cap_usd: cap } : {}),
        ...(budget !== undefined ? { conversation_budget_usd: budget } : {}),
        global_instructions: $("globalInstructions").value,
        ...($("edinetKey").value.trim()
          ? { edinet_api_key: $("edinetKey").value.trim() }
          : {}),
      });
      $("edinetKey").value = "";
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
      $("keyStatus").textContent =
        "No key yet — you're in demo mode with sample companies.";
      setStatus("Key removed — back to demo mode.", "info");
      onSaved();
    } catch (e) {
      setStatus(`Remove failed: ${e.message || e}`, "error");
    }
  });

  const connectGo = $("connectOpencodeGo");
  if (connectGo) {
    connectGo.addEventListener("click", async () => {
      const st = $("importOpencodeStatus");
      if (st) st.textContent = "Connecting OpenCode Go…";
      try {
        const res = await call("connect_opencode_go");
        await refreshSubscriptionProviders();
        populateProviders();
        populateProviderBaseList();
        if (!res.needs_auth && res.base_url) {
          setProviderFromBase(res.base_url, res.model || "");
        }
        if (res.needs_auth) {
          startOpenCodeLoginPoll(() => connectGo.click());
          if (st) {
            st.textContent =
              res.guidance || "Authenticate through OpenCode or OMP, then reconnect.";
          }
          setStatus(
            res.guidance || "Authenticate through OpenCode or OMP, then reconnect.",
            "info",
          );
        } else {
          stopOpenCodeLoginPoll();
          $("keyStatus").textContent = "Subscription credential stays in OMP.";
          if (st) st.textContent = `Connected from ${res.source || "local"}.`;
          setStatus("OpenCode Go connected.", "info");
          onSaved();
        }
      } catch (e) {
        if (st) st.textContent = "";
        setStatus(`OpenCode Connect failed: ${e.message || e}`, "error");
      }
    });
  }

  const importGo = $("importOpencodeGo");
  if (importGo) {
    importGo.addEventListener("click", async () => {
      const st = $("importOpencodeStatus");
      if (st) st.textContent = "Importing…";
      try {
        const res = await call("import_opencode_go_key");
        await refreshSubscriptionProviders();
        populateProviders();
        populateProviderBaseList();
        setProviderFromBase(res.base_url, res.model);
        $("keyStatus").textContent = "Subscription credential stays in OMP.";
        if (st) st.textContent = `Connected through OMP from ${res.source}.`;
        setStatus("OpenCode Go connected through OMP.", "info");
        onSaved();
      } catch (e) {
        if (st) st.textContent = "";
        setStatus(`Import failed: ${e.message || e}`, "error");
      }
    });
  }


  const connectCursor = $("connectCursorOmp");
  if (connectCursor) {
    connectCursor.addEventListener("click", async () => {
      const stEl = $("cursorProbeStatus");
      if (stEl) stEl.textContent = "Connecting Cursor via omp…";
      try {
        const res = await call("connect_cursor_omp");
        if (res.chat_ready && res.base_url) {
          stopCursorLoginPoll();
          await refreshSubscriptionProviders();
          populateProviders();
          populateProviderBaseList();
          setProviderFromBase(res.base_url, res.model);
          if (res.model && $("modelSelect")) $("modelSelect").value = res.model;
          await refreshModelOptionsForProvider("cursor", res.model || "");
          if (stEl)
            stEl.textContent = `Cursor chat ready via ${res.base_url} (model ${res.model}).`;
          setStatus("Cursor connected through local OMP gateway.", "info");
          onSaved();
          return;
        }
        if (stEl) stEl.textContent = res.guidance || "Waiting for omp Cursor login…";
        setStatus(res.guidance || "Complete Cursor login in the omp window.", "info");
        startCursorLoginPoll(async () => {
          try {
            const wired = await applyCursorProvider();
            await refreshModelOptionsForProvider("cursor", wired.model || "");
            if (stEl)
              stEl.textContent = `Cursor chat ready via ${wired.base_url} (model ${wired.model}).`;
            setStatus("Cursor connected through local OMP gateway.", "info");
            onSaved();
          } catch (err) {
            if (stEl) stEl.textContent = (err && err.message) || String(err);
          }
        });
      } catch (e) {
        if (stEl) stEl.textContent = (e && e.message) || String(e);
        setStatus(`Connect Cursor failed: ${e.message || e}`, "error");
      }
    });
  }

  const useCursor = $("useCursorOmp");
  if (useCursor) {
    useCursor.addEventListener("click", async () => {
      const st = $("cursorProbeStatus");
      if (st) st.textContent = "Starting OMP Cursor gateway…";
      try {
        const res = await applyCursorProvider();
        await refreshModelOptionsForProvider("cursor", res.model || "");
        if (st)
          st.textContent = `Cursor chat ready via ${res.base_url} (model ${res.model}).`;
        setStatus("Cursor wired through local OMP gateway. Save to keep.", "info");
        onSaved();
      } catch (e) {
        if (st) st.textContent = (e && e.message) || String(e);
        const reconnect = $("connectCursorOmp");
        if (reconnect) reconnect.hidden = false;
        setStatus(`Existing Cursor login failed: ${e.message || e}. Reconnect to authenticate again.`, "error");
      }
    });
  }

  const probeCursor = $("probeCursorModels");
  if (probeCursor) {
    probeCursor.addEventListener("click", async () => {
      const st = $("cursorProbeStatus");
      if (st) st.textContent = "Probing Cursor models via omp…";
      try {
        const models = await refreshModelOptionsForProvider("cursor");
        const sample = models.slice(0, 3).map((m) => m.id).join(", ") || "—";
        if (st)
          st.textContent =
            `Cursor models: ${models.length} (e.g. ${sample}). Use Cursor or select Provider → Cursor to chat.`;
        setStatus(`Cursor probe ok — ${models.length} models.`, "info");
      } catch (e) {
        if (st) st.textContent = (e && e.message) || String(e);
        setStatus(`Cursor probe failed: ${e.message || e}`, "error");
      }
    });
  }
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
      el.textContent = "Enter the helper command first (for example: roam mcp serve).";
      return;
    }
    el.textContent = "Testing…";
    try {
      const parts = cmd.split(/\s+/);
      const res = await call("test_mcp", {
        command: parts[0],
        args: parts.slice(1),
      });
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
      await call("save_settings", {
        api_key: $("apiKey").value,
        model: "",
        base_url: $("baseUrl").value,
      });
      $("apiKey").value = "";
      const models = await refreshModelOptionsForProvider($("providerSelect")?.value || "");
      await populateModelCatalogList();
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
        cap.native_tools
          ? `${cap.model_id} looks good — it can use the analyst's tools.`
          : cap.model_id.startsWith("cursor/")
            ? `${cap.model_id} is direct/synthesis-only via OMP. For tool-backed financial runs, switch to OpenCode Go in Settings.`
            : `${cap.model_id} can't use tools — research and models won't work well with it.`,
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
