import { test } from "node:test";
import assert from "node:assert/strict";
import { createStore, reduce, updateDraft, setConversation, markVerified } from "../js/reducer.mjs";
// memory notices, error handling, stop flow, message construction.


function ev(type, over = {}) {
  return { event: { type, ...over }, durability: "ephemeral", run_id: "r1", conversation_id: "c1" };
}

test("createStore initial state", () => {
  const s = createStore();
  assert.equal(s.conversationId, null);
  assert.equal(s.streaming, false);
  assert.equal(s.messages.length, 0);
  assert.equal(s.draftText, "");
});

test("createStore has no schema error", () => {
  assert.equal(createStore().schemaError, null);
});

test("newer schema_version is rejected with a recoverable message", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  const before = s.streaming;
  s = reduce(s, { schema_version: 999, event: { type: "RunStarted" }, run_id: "r1", conversation_id: "c1" });
  assert.ok(s.schemaError, "sets a recoverable schema error");
  assert.match(s.schemaError, /refresh/i);
  // The unknown event is NOT reduced (state otherwise preserved).
  assert.equal(s.streaming, before);
});

test("known schema_version is accepted", () => {
  let s = reduce(createStore(), { schema_version: 2, event: { type: "RunStarted" }, run_id: "r1", conversation_id: "c1" });
  assert.equal(s.schemaError, null);
  assert.equal(s.streaming, true);
});

test("RunStarted sets streaming and status", () => {
  let s = createStore();
  s = reduce(s, ev("RunStarted"));
  assert.equal(s.streaming, true);
  assert.equal(s.runStatus, "running");
  assert.equal(s.activeRunId, "r1");
  assert.equal(s.phaseLabel, "Preparing…");
});

test("PhaseChanged updates phase label", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("PhaseChanged", { detail: "Gathering data…" }));
  assert.equal(s.phaseLabel, "Gathering data…");
});

test("AssistantTextDelta accumulates draft text", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("AssistantTextDelta", { text: "The answer is " }));
  s = reduce(s, ev("AssistantTextDelta", { text: "42." }));
  assert.equal(s.draftText, "The answer is 42.");
});

test("AssistantCheckpoint commits message and clears draft", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("UserMessage", { text: "What is the answer?" }));
  s = reduce(s, ev("AssistantTextDelta", { text: "42" }));
  assert.equal(s.draftText, "42");
  s = reduce(s, ev("AssistantCheckpoint", { text: "The final answer is 42." }));
  assert.equal(s.draftText, "");
  assert.equal(s.messages.length, 2); // user + assistant
  assert.equal(s.messages[1].role, "assistant");
  assert.equal(s.messages[1].text, "The final answer is 42.");
});

test("UserMessage adds to messages", () => {
  let s = createStore();
  s = reduce(s, ev("UserMessage", { text: "Build a DCF for AAPL" }));
  assert.equal(s.messages.length, 1);
  assert.equal(s.messages[0].role, "user");
  assert.equal(s.messages[0].text, "Build a DCF for AAPL");
  assert.equal(s.lastQuestion, "Build a DCF for AAPL");
});

test("Tool events affect phase label", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("ToolQueued", { name: "web_search" }));
  s = reduce(s, ev("ToolStarted", { name: "web_search", label: "Searching 3 queries" }));
  assert.equal(s.phaseLabel, "Searching 3 queries");
  s = reduce(s, ev("ToolSucceeded"));
  assert.equal(s.phaseLabel, null);
});

test("RunCompleted ends streaming", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("RunCompleted"));
  assert.equal(s.streaming, false);
  assert.equal(s.runStatus, "completed");
  assert.equal(s.phaseLabel, null);
});

test("RunFailed sets failed status", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("RunFailed", { error: "API timeout" }));
  assert.equal(s.streaming, false);
  assert.equal(s.runStatus, "failed");
  assert.equal(s.phaseLabel, "API timeout");
});

test("RunCancelled sets cancelled", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("RunCancelled"));
  assert.equal(s.runStatus, "cancelled");
  assert.equal(s.streaming, false);
});

test("RunBudgetLimited sets budget limit phase", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("RunBudgetLimited"));
  assert.equal(s.runStatus, "budget_limited");
  assert.equal(s.phaseLabel, "Budget limit reached");
});

test("ApprovalRequested creates approval request", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("ApprovalRequested", { tool_call_id: "t1", name: "export", query: "file.xlsx" }));
  assert.ok(s.approvalRequest);
  assert.equal(s.approvalRequest.name, "export");
  assert.equal(s.phaseLabel, "Awaiting approval…");
});

test("ApprovalResolved clears approval request", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("ApprovalRequested", { tool_call_id: "t1" }));
  s = reduce(s, ev("ApprovalResolved", { response: "approve_once" }));
  assert.equal(s.approvalRequest, null);
  assert.equal(s.phaseLabel, "Approved");
});

test("MemoryUpdated sets lastAnnounce", () => {
  let s = reduce(createStore(), ev("MemoryUpdated", { count: 3 }));
  assert.equal(s.lastAnnounce, "Memory updated · 3");
});

test("Error sets lastAnnounce with detail", () => {
  let s = reduce(createStore(), ev("Error", { detail: "Connection lost" }));
  assert.equal(s.lastAnnounce, "Connection lost");
});

test("StopRequested sets stopping", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("StopRequested"));
  assert.equal(s.stopping, true);
  assert.equal(s.phaseLabel, "Stopping…");
});

test("StopComplete ends streaming", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("StopRequested"));
  s = reduce(s, ev("StopComplete"));
  assert.equal(s.stopping, false);
  assert.equal(s.streaming, false);
  assert.equal(s.runStatus, "cancelled");
});

test("RunInterrupted sets interrupted status", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("RunInterrupted"));
  assert.equal(s.runStatus, "interrupted");
});

test("updateDraft replaces draft text", () => {
  let s = updateDraft(createStore(), "custom text");
  assert.equal(s.draftText, "custom text");
});

test("setConversation resets messages", () => {
  let s = setConversation(createStore(), "conv2");
  assert.equal(s.conversationId, "conv2");
  assert.equal(s.messages.length, 0);
});

test("markVerified marks last assistant message", () => {
  let s = reduce(createStore(), ev("UserMessage", { text: "Q" }));
  s = reduce(s, ev("AssistantCheckpoint", { text: "A" }));
  s = markVerified(s);
  assert.equal(s.messages[1].verified, true);
});

test("unknown event returns state unchanged", () => {
  const s = createStore();
  const next = reduce(s, ev("UnknownEventType"));
  assert.equal(next, s); // same reference for no-op
});

test("lowercase event types work (OMP style)", () => {
  let s = reduce(createStore(), ev("run_started"));
  assert.equal(s.streaming, true);
  s = reduce(s, ev("run_completed"));
  assert.equal(s.runStatus, "completed");
});

test("draft text survives multiple deltas then checkpoint", () => {
  let s = reduce(createStore(), ev("RunStarted"));
  s = reduce(s, ev("AssistantTextDelta", { text: "Step 1: " }));
  s = reduce(s, ev("AssistantTextDelta", { text: "analyze." }));
  s = reduce(s, ev("AssistantTextDelta", { text: " Step 2: conclude." }));
  assert.equal(s.draftText, "Step 1: analyze. Step 2: conclude.");
  s = reduce(s, ev("AssistantCheckpoint", {})); // no explicit text
  assert.equal(s.draftText, "");
  assert.equal(s.messages.length, 1);
  // Without explicit text in checkpoint, uses draftText as fallback.
});

test("full message order preserved", () => {
  let s = createStore();
  s = reduce(s, ev("UserMessage", { text: "Q1" }));
  s = reduce(s, ev("AssistantCheckpoint", { text: "A1" }));
  s = reduce(s, ev("UserMessage", { text: "Q2" }));
  s = reduce(s, ev("AssistantCheckpoint", { text: "A2" }));
  assert.equal(s.messages.length, 4);
  assert.equal(s.messages[0].text, "Q1");
  assert.equal(s.messages[1].text, "A1");
  assert.equal(s.messages[2].text, "Q2");
  assert.equal(s.messages[3].text, "A2");
});

test("markVerified with no assistant message is no-op", () => {
  let s = createStore();
  s = markVerified(s); // no crash
  assert.equal(s.messages.length, 0);
});

// ── Task 2.1: real Rust envelope shape + replay idempotency ──────────────

function durable(kind, seq, payload = {}) {
  return {
    schema_version: 2,
    event: { kind, payload },
    sequence: seq,
    run_id: "r1",
    conversation_id: "c1",
  };
}

test("reduces the Rust envelope shape (kind + payload)", () => {
  let s = reduce(createStore(), durable("run_started", 1));
  assert.equal(s.streaming, true);
  assert.equal(s.runStatus, "running");
  s = reduce(s, durable("phase_changed", 2, { phase: "Executing" }));
  assert.equal(s.phaseLabel, "Executing");
  s = reduce(s, durable("memory_updated", 3, { count: 3 }));
  assert.equal(s.lastAnnounce, "Memory updated · 3");
  s = reduce(s, durable("run_completed", 4, { stop: { kind: "end_turn" }, partial: false }));
  assert.equal(s.runStatus, "completed");
  assert.equal(s.streaming, false);
});

test("PlanUpdated stores the whole plan payload", () => {
  const plan = { objective: "NVDA earnings", assumptions: [], steps: [{ id: "s1", label: "Read 10-K", status: "pending" }], version: 1 };
  let s = reduce(createStore(), durable("run_started", 1));
  s = reduce(s, durable("plan_updated", 2, plan));
  assert.equal(s.plan.objective, "NVDA earnings");
  assert.equal(s.plan.steps.length, 1);
});

test("durable replay by (run_id, sequence) is idempotent", () => {
  const seqEvents = [
    durable("run_started", 1),
    durable("phase_changed", 2, { phase: "Executing" }),
    durable("tool_started", 3, { tool_call_id: "t1" }),
    durable("tool_succeeded", 4, { tool_call_id: "t1" }),
    durable("run_completed", 5, { stop: { kind: "end_turn" }, partial: false }),
  ];
  // Live: apply once.
  let live = createStore();
  for (const e of seqEvents) live = reduce(live, e);
  // Reload: snapshot (empty) then replay the SAME durable sequence.
  let reload = createStore();
  for (const e of seqEvents) reload = reduce(reload, e);
  // And re-deliver every event a second time (gap-close overlap / double emit).
  for (const e of seqEvents) reload = reduce(reload, e);
  assert.equal(JSON.stringify(reload), JSON.stringify(live), "reload == live after replay");
});

test("re-delivered older sequence is a no-op", () => {
  let s = reduce(createStore(), durable("run_started", 1));
  s = reduce(s, durable("phase_changed", 3, { phase: "Synthesizing" }));
  const before = JSON.stringify(s);
  // A late/duplicate seq 2 (<= applied 3) must not mutate state.
  s = reduce(s, durable("phase_changed", 2, { phase: "STALE" }));
  assert.equal(JSON.stringify(s), before);
  assert.equal(s.phaseLabel, "Synthesizing");
});
