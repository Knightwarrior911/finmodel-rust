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
  ctx.emit("chat_progress", { conversation_id: "c", run_id: "r", text: "Searching 3 queries" });
  await tick();
  assert.equal(document.getElementById("chatProgress").textContent, "");
});

test("chat_delta for no active turn creates no assistant node", async () => {
  const { ctx } = await bootChat();
  ctx.emit("chat_delta", { conversation_id: "c", run_id: "r", text: "hello" });
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
