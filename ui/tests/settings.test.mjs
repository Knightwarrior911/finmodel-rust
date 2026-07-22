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
