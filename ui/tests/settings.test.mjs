import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

test("Scheduled tab lists follow-ups and cancel works", async () => {
  const ctx = setupDom();
  let cancelled = null;
  ctx.invokeHandlers.load_settings = async () => ({ has_key: true, model: "m" });
  ctx.invokeHandlers.memory_list = async () => [];
  ctx.invokeHandlers.skills_list = async () => [];
  ctx.invokeHandlers.schedules_list = async () => [
    {
      id: "sch-1",
      conversation_id: "c1",
      recurrence: "daily",
      next_due: "2026-07-20T09:00:00Z",
      scope_json: JSON.stringify({ prompt: "morning brief on watchlist" }),
      status: "pending",
      last_outcome: null,
    },
  ];
  ctx.invokeHandlers.schedule_cancel = async (args) => {
    cancelled = args.id;
    return true;
  };
  const settings = await importModule("settings.mjs");
  settings.initSettings({ onSaved: () => {} });
  await settings.openSettings();
  await tick();
  settings.selectSettingsTab("scheduled");
  const panel = document.getElementById("settingsPanel-scheduled");
  assert.equal(panel.hidden, false, "scheduled panel visible");
  const row = panel.querySelector(".schedule-row");
  assert.ok(row, "schedule row rendered");
  assert.match(row.textContent, /morning brief on watchlist/);
  assert.match(row.textContent, /every day/);
  // Cancel round-trips and refreshes to the empty state.
  ctx.invokeHandlers.schedules_list = async () => [];
  row.querySelector(".schedule-cancel").click();
  await tick();
  await tick();
  assert.equal(cancelled, "sch-1");
  assert.match(
    document.getElementById("schedulesList").textContent,
    /Nothing scheduled/,
  );
});


test("Cursor subscription button wires the local gateway", async () => {
  const ctx = setupDom();
  ctx.invokeHandlers.load_settings = async () => ({
    has_key: true,
    model: "gpt-5.6",
    base_url: "https://openrouter.ai/api/v1",
    auto_route_vision: true,
  });
  ctx.invokeHandlers.subscription_providers_status = async () => ({
    enabled: true,
    providers: [
      {
        id: "cursor",
        name: "Cursor (via OMP gateway)",
        base: "http://127.0.0.1:4000/v1",
        chat_ready: true,
      },
    ],
    cursor: { chat_ready: true, available: true, reason: "" },
    opencode: { chat_ready: false, reason: "needs key" },
  });
  ctx.invokeHandlers.list_models = async () => [];
  ctx.invokeHandlers.memory_list = async () => [];
  ctx.invokeHandlers.skills_list = async () => [];
  ctx.invokeHandlers.agents_list = async () => [];
  ctx.invokeHandlers.schedules_list = async () => [];
  ctx.invokeHandlers.use_cursor_omp = async () => ({
    base_url: "http://127.0.0.1:4000/v1",
    model: "cursor/claude-4.6-sonnet-medium",
    chat_ready: true,
  });
  const settings = await importModule("settings.mjs");
  settings.initSettings({ onSaved: () => {} });
  await settings.openSettings();
  await tick();
  const use = document.getElementById("useCursorOmp");
  assert.equal(use.hidden, false, "Use Cursor is shown when OAuth is ready");
  use.click();
  await tick();
  assert.ok(
    ctx.invokeLog.some((entry) => entry.name === "use_cursor_omp"),
    "Use Cursor invokes the gateway command",
  );
  assert.match(
    document.getElementById("cursorProbeStatus").textContent,
    /Cursor chat ready via http:\/\/127\.0\.0\.1:4000\/v1/,
  );
});


test("OpenCode auth handoff preserves settings until credentials exist", async () => {
  const ctx = setupDom();
  ctx.invokeHandlers.load_settings = async () => ({
    has_key: false,
    model: "openrouter/auto",
    base_url: "https://openrouter.ai/api/v1",
    auto_route_vision: true,
  });
  ctx.invokeHandlers.subscription_providers_status = async () => ({
    enabled: true,
    providers: [
      {
        id: "opencode-go",
        name: "OpenCode Go",
        base: "http://127.0.0.1:4000/v1",
        chat_ready: false,
      },
    ],
    cursor: { chat_ready: false, available: false, reason: "" },
    opencode: { chat_ready: false, reason: "needs authentication" },
  });
  ctx.invokeHandlers.connect_opencode_go = async () => ({
    base_url: "http://127.0.0.1:4000/v1",
    needs_auth: true,
    guidance: "Authenticate through OpenCode or OMP, then reconnect.",
  });
  ctx.invokeHandlers.list_models = async () => [];
  ctx.invokeHandlers.memory_list = async () => [];
  ctx.invokeHandlers.skills_list = async () => [];
  ctx.invokeHandlers.agents_list = async () => [];
  ctx.invokeHandlers.schedules_list = async () => [];

  const settings = await importModule("settings.mjs");
  settings.initSettings({ onSaved: () => {} });
  await settings.openSettings();
  await tick();

  const provider = document.getElementById("providerSelect");
  assert.equal(provider.value, "openrouter");
  document.getElementById("connectOpencodeGo").click();
  await tick();
  await tick();

  assert.equal(provider.value, "openrouter");
});


test("provider picker keeps Cursor, OpenRouter, and OpenCode Go catalogs separate", async () => {
  const ctx = setupDom();
  let saved = null;
  ctx.invokeHandlers.load_settings = async () => ({
    has_key: true,
    model: "opencode-go/model-b",
    base_url: "http://127.0.0.1:4000/v1",
    auto_route_vision: true,
  });
  ctx.invokeHandlers.subscription_providers_status = async () => ({
    enabled: true,
    providers: [
      { id: "openrouter", name: "OpenRouter", base: "https://openrouter.ai/api/v1", chat_ready: true },
      { id: "opencode-go", name: "OpenCode Go", base: "http://127.0.0.1:4000/v1", chat_ready: true },
      { id: "cursor", name: "Cursor (via OMP gateway)", base: "http://127.0.0.1:4000/v1", chat_ready: true },
    ],
    cursor: { chat_ready: true, available: true, reason: "" },
    opencode: { chat_ready: true, reason: "" },
  });
  ctx.invokeHandlers.list_models = async (args = {}) => {
    if (args.provider_id === "openrouter") return [{ id: "openrouter/model-a", name: "OpenRouter A" }];
    if (args.provider_id === "opencode-go") return [{ id: "opencode-go/model-b", name: "OpenCode Go B" }];
    return [];
  };
  ctx.invokeHandlers.connect_opencode_go = async () => ({
    base_url: "http://127.0.0.1:4000/v1",
    model: "opencode-go/model-b",
    needs_auth: false,
    credential_owner: "omp",
  });
  ctx.invokeHandlers.connect_cursor_omp = async () => ({
    base_url: "http://127.0.0.1:4000/v1",
    model: "cursor/claude-4.6-sonnet-medium",
    chat_ready: true,
  });
  ctx.invokeHandlers.probe_cursor_models = async () => ({
    ok: true,
    count: 2,
    models: [
      { id: "cursor/claude-4.6-sonnet-medium", name: "Claude 4.6 Sonnet Medium" },
      { id: "cursor/cursor-grok-4.5-medium", name: "Grok 4.5 Medium" },
    ],
  });
  ctx.invokeHandlers.use_cursor_omp = async () => ({
    base_url: "http://127.0.0.1:4000/v1",
    model: "cursor/claude-4.6-sonnet-medium",
    chat_ready: true,
  });
  ctx.invokeHandlers.memory_list = async () => [];
  ctx.invokeHandlers.skills_list = async () => [];
  ctx.invokeHandlers.agents_list = async () => [];
  ctx.invokeHandlers.schedules_list = async () => [];
  ctx.invokeHandlers.save_settings = async (args) => { saved = args; return "{}"; };
  ctx.invokeHandlers.test_model = async () => ({ model_id: "selected" });

  const settings = await importModule("settings.mjs");
  settings.initSettings({ onSaved: () => {} });
  await settings.openSettings();
  await tick();

  const provider = document.getElementById("providerSelect");
  const models = document.getElementById("modelSelect");
  assert.equal(provider.value, "opencode-go", "model prefix disambiguates shared gateway");
  provider.value = "cursor";
  provider.dispatchEvent(new window.Event("change", { bubbles: true }));
  await tick();
  await tick();
  assert.deepEqual(
    [...models.options].map((o) => o.value),
    ["cursor/claude-4.6-sonnet-medium", "cursor/cursor-grok-4.5-medium"],
  );

  models.value = "cursor/cursor-grok-4.5-medium";
  provider.value = "openrouter";
  provider.dispatchEvent(new window.Event("change", { bubbles: true }));
  await tick();
  await tick();
  assert.deepEqual([...models.options].map((o) => o.value), ["openrouter/model-a"]);

  provider.value = "opencode-go";
  provider.dispatchEvent(new window.Event("change", { bubbles: true }));
  await tick();
  await tick();
  assert.deepEqual([...models.options].map((o) => o.value), ["opencode-go/model-b"]);

  provider.value = "cursor";
  provider.dispatchEvent(new window.Event("change", { bubbles: true }));
  await tick();
  await tick();
  models.value = "cursor/cursor-grok-4.5-medium";
  document.getElementById("saveSettings").click();
  await tick();
  assert.equal(saved.model, "cursor/cursor-grok-4.5-medium");
});
