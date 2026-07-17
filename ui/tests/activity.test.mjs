// activity.test.mjs — Tool activity reducer + renderer tests (Phase D).
//
// Tests cover:
// - State reduction: every event type produces correct status
// - Partial updates: only changed tool_call_ids
// - Batch grouping
// - Approval flow
// - Interrupted run cascades
// - DOM rendering matches expected structure

import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

/** Convenience: create an agent event envelope. */
function ev(type, over = {}) {
  return { event: { type, ...over }, durability: "ephemeral" };
}

async function boot() {
  setupDom();
  const act = await importModule("activity.mjs");
  const state = act.createState();
  return { act, state };
}

test("createState returns empty maps", async () => {
  const { act: _, state } = await boot();
  assert.equal(state.byId.size, 0);
  assert.equal(state.byBatch.size, 0);
  assert.equal(state.parentOf.size, 0);
  assert.equal(state.lastAnnounce, null);
});

test("tool_started creates running activity", async () => {
  const { act, state } = await boot();
  const changed = act.reduce(state, ev("ToolStarted", { tool_call_id: "t1", name: "web_search", detail: "NVDA" }));
  assert.deepEqual(changed, ["t1"]);
  assert.equal(state.byId.size, 1);
  const t = state.byId.get("t1");
  assert.equal(t.name, "web_search");
  assert.equal(t.status, "running");
  assert.equal(t.query, "NVDA");
  assert.equal(t.attempts, 0);
});

test("tool_succeeded transitions status", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1", name: "get_quote" }));
  const changed = act.reduce(state, ev("ToolSucceeded", { tool_call_id: "t1", summary: "AAPL $198" }));
  assert.deepEqual(changed, ["t1"]);
  assert.equal(state.byId.get("t1").status, "success");
  assert.equal(state.byId.get("t1").detail, "AAPL $198");
  assert.ok(state.byId.get("t1").finished_at != null);
});

test("tool_failed sets error", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  act.reduce(state, ev("ToolFailed", { tool_call_id: "t1", error: "API timeout", detail: "OpenRouter" }));
  assert.equal(state.byId.get("t1").status, "error");
  assert.equal(state.byId.get("t1").error, "API timeout");
});

test("tool_cancelled sets cancelled status", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  act.reduce(state, ev("ToolCancelled", { tool_call_id: "t1" }));
  assert.equal(state.byId.get("t1").status, "cancelled");
});

test("tool_progress appends to bounded tail", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  for (let i = 0; i < 8; i++) {
    act.reduce(state, ev("ToolProgress", { tool_call_id: "t1", text: `line${i}` }));
  }
  const tail = state.byId.get("t1").tail;
  assert.equal(tail.length, 6); // bounded
  assert.equal(tail[0], "line2"); // earliest dropped
  assert.equal(tail[5], "line7");
});

test("approval_requested sets awaiting_approval and auto-expands", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ApprovalRequested", { tool_call_id: "a1", name: "export", query: "export.xlsx" }));
  const t = state.byId.get("a1");
  assert.equal(t.status, "awaiting_approval");
  assert.ok(t.expanded);
});

test("approval_resolved marks success", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ApprovalRequested", { tool_call_id: "a1" }));
  act.reduce(state, ev("ApprovalResolved", { tool_call_id: "a1", response: "approve_once" }));
  assert.equal(state.byId.get("a1").status, "success");
});

test("approval_deny marks cancelled", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ApprovalRequested", { tool_call_id: "a1" }));
  act.reduce(state, ev("ApprovalResolved", { tool_call_id: "a1", response: "deny" }));
  assert.equal(state.byId.get("a1").status, "cancelled");
});

test("batch_id groups activities", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1", batch_id: "b1" }));
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t2", batch_id: "b1" }));
  assert.deepEqual(state.byBatch.get("b1"), ["t1", "t2"]);
  assert.equal(state.parentOf.get("t1"), "b1");
  assert.equal(state.parentOf.get("t2"), "b1");
});

test("terminal run event interrupts running activities", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t2" }));
  act.reduce(state, ev("ToolSucceeded", { tool_call_id: "t2" }));
  act.reduce(state, ev("RunCancelled", {}));
  assert.equal(state.byId.get("t1").status, "interrupted");
  assert.equal(state.byId.get("t2").status, "success"); // unchanged
});

test("render creates DOM elements for activities", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1", name: "get_quote" }));
  act.reduce(state, ev("ToolSucceeded", { tool_call_id: "t1", summary: "AAPL $198" }));

  const container = document.createElement("div");
  act.render(container, state);
  await tick();

  assert.equal(container.children.length, 1);
  const row = container.firstChild;
  assert.ok(row.classList.contains("act-row"));
  assert.ok(row.classList.contains("act-success"));
  assert.equal(row.dataset.toolCallId, "t1");
  assert.ok(row.querySelector(".act-badge"));
  assert.ok(row.querySelector(".act-header"));
});

test("render partial update upserts only changed ids", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1", name: "web_search" }));
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t2", name: "read_page" }));

  const container = document.createElement("div");
  act.render(container, state);
  await tick();

  // Now change only t1.
  act.reduce(state, ev("ToolSucceeded", { tool_call_id: "t1" }));
  act.render(container, state, ["t1"]);
  await tick();

  assert.equal(container.children.length, 2);
  assert.ok(container._els.get("t1").classList.contains("act-success"));
});

test("cleanupRun removes activities for a run", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "run1_t1" }));
  act.reduce(state, ev("ToolStarted", { tool_call_id: "run1_t2" }));
  act.reduce(state, ev("ToolStarted", { tool_call_id: "run2_t1" }));
  const removed = act.cleanupRun(state, "run1");
  assert.equal(removed.length, 2);
  assert.equal(state.byId.size, 1);
  assert.ok(state.byId.has("run2_t1"));
});

test("resetState clears everything", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  act.resetState(state);
  assert.equal(state.byId.size, 0);
  assert.equal(state.byBatch.size, 0);
});

test("render creates approval buttons for awaiting_approval", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ApprovalRequested", { tool_call_id: "a1", name: "export", query: "report.xlsx" }));

  const container = document.createElement("div");
  act.render(container, state);
  await tick();

  const row = container.firstChild;
  assert.ok(row.querySelector(".act-btn-approve"));
  assert.ok(row.querySelector(".act-btn-deny"));
});

test("render shows duration for completed activities", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  act.reduce(state, ev("ToolSucceeded", { tool_call_id: "t1" }));

  const container = document.createElement("div");
  act.render(container, state);
  await tick();

  const dur = container.querySelector(".act-duration");
  assert.ok(dur);
  assert.ok(dur.textContent.match(/^\d+s/)); // at least 0s
});

test("render shows error in detail area", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  act.reduce(state, ev("ToolFailed", { tool_call_id: "t1", error: "timeout" }));

  const container = document.createElement("div");
  act.render(container, state);
  await tick();

  const err = container.querySelector(".act-errtext");
  assert.ok(err);
  assert.equal(err.textContent, "timeout");
});

test("render shows tail content", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("ToolStarted", { tool_call_id: "t1" }));
  act.reduce(state, ev("ToolProgress", { tool_call_id: "t1", text: "Searching..." }));

  const container = document.createElement("div");
  act.render(container, state);
  await tick();

  const tail = container.querySelector(".act-tail");
  assert.ok(tail);
  assert.equal(tail.textContent, "Searching...");
});

test("reduce handles lowercase event types (OMP style)", async () => {
  const { act, state } = await boot();
  act.reduce(state, ev("tool_started", { tool_call_id: "t1", name: "build_model" }));
  assert.equal(state.byId.get("t1").status, "running");
  assert.equal(state.byId.get("t1").name, "build_model");
});
