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
