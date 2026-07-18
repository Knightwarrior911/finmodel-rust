import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

async function bootChat() {
  const ctx = setupDom();
  const chat = await importModule("chat.mjs");
  chat.initChat({ onConversationChanged: () => {} });
  return { ctx, chat };
}

test("chat_progress before any run is ignored (no active turn)", async () => {
  const { ctx } = await bootChat();
  ctx.emit("chat_progress", {
    conversation_id: "c",
    run_id: "r",
    text: "Searching 3 queries",
  });
  await tick();
  assert.equal(document.getElementById("chatProgress").textContent, "");
});

test("stray text delta with no active turn creates no assistant node", async () => {
  const { ctx } = await bootChat();
  // Text deltas now ride the single agent_event channel (Task 2.1); a delta with
  // no active run has no per-run listener, so it must not create a phantom node.
  ctx.emit("agent_event", {
    run_id: "r",
    conversation_id: "c",
    event: { kind: "assistant_text_delta", payload: { text: "hello" } },
  });
  await tick();
  assert.equal(document.querySelectorAll("#chatScroll .prose").length, 0);
});

test("progress region is polite + atomic; alert region is assertive", async () => {
  await bootChat();
  const p = document.getElementById("chatProgress");
  assert.equal(p.getAttribute("role"), "status");
  assert.equal(p.getAttribute("aria-live"), "polite");
  assert.equal(p.getAttribute("aria-atomic"), "true");
  const a = document.getElementById("chatAlert");
  assert.equal(a.getAttribute("role"), "alert");
  assert.equal(a.getAttribute("aria-live"), "assertive");
});

test("newChat clears alert and resets reader-open", async () => {
  const { chat } = await bootChat();
  document.body.classList.add("reader-open");
  const a = document.getElementById("chatAlert");
  a.hidden = false;
  a.textContent = "stale error";
  chat.newChat();
  assert.equal(a.hidden, true, "alert cleared");
  assert.ok(!document.body.classList.contains("reader-open"), "reader reset");
});

test("load failure announces + retains, offers keyboard retry (no silent new chat)", async () => {
  const { ctx, chat } = await bootChat();
  ctx.invokeHandlers.load_conversation = async () => {
    throw new Error("corrupt file");
  };
  await chat.loadConversation("123-abcd");
  await tick();
  const a = document.getElementById("chatAlert");
  assert.equal(a.hidden, false, "alert shown");
  assert.match(a.textContent, /corrupt file/);
  const retry = a.querySelector("button");
  assert.ok(retry, "keyboard-reachable retry present");
});

test("pause surfaces a Resume affordance that relaunches via agent_resume", async () => {
  const { ctx } = await bootChat();
  let resumeArg = null;
  ctx.invokeHandlers.agent_send = async () => ({
    conversation_id: "c1",
    run_id: "run-1",
  });
  ctx.invokeHandlers.agent_pause = async () => true;
  ctx.invokeHandlers.agent_resume = async (args) => {
    resumeArg = args;
    return "run-2";
  };

  document.getElementById("chatInput").value = "do an earnings review for NVDA";
  document.getElementById("chatSend").click();
  await tick();
  await tick();

  // Pause is visible during a run; clicking it requests a resumable interrupt.
  const pauseBtn = document.getElementById("chatPause");
  assert.equal(pauseBtn.hidden, false, "pause button visible during a run");
  pauseBtn.click();
  await tick();

  // Backend ends the run interrupted (resumable), not cancelled.
  ctx.emit("agent_event", {
    run_id: "run-1",
    conversation_id: "c1",
    event: { kind: "run_interrupted", payload: {} },
  });
  await tick();
  await tick();

  const alert = document.getElementById("chatAlert");
  assert.equal(alert.hidden, false, "paused recovery region shown");
  const resumeBtn = [...alert.querySelectorAll("button")].find(
    (b) => b.textContent === "Resume",
  );
  assert.ok(
    resumeBtn,
    "Resume affordance present (not the plain Stopped recovery)",
  );

  // Resuming relaunches the interrupted run through agent_resume.
  resumeBtn.click();
  await tick();
  await tick();
  assert.deepEqual(
    resumeArg,
    { interrupted_run_id: "run-1" },
    "resume targets the interrupted run id",
  );
  // Let the resumed run reach terminal so no wait is left dangling.
  ctx.emit("agent_event", {
    run_id: "run-2",
    conversation_id: "c1",
    event: { kind: "run_completed", payload: {} },
  });
  await tick();
});
