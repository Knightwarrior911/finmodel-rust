// memory.test.mjs — memory management surface (Task 7.2): pin/unpin reversibility.
import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

async function bootWithMemories(mems) {
  const ctx = setupDom();
  ctx.invokeHandlers.load_settings = async () => ({
    has_key: true,
    model: "m",
    edgar_contact: "",
    out_dir: "",
    mcp_command: "",
    mcp_args: [],
    version: "0.4.0",
    model_capability: null,
  });
  ctx.invokeHandlers.memory_list = async () => mems;
  ctx.invokeHandlers.memory_pin = async () => ({ ok: true });
  ctx.invokeHandlers.skills_list = async () => [];
  const settings = await importModule("settings.mjs");
  settings.initSettings({ onSaved: () => {} });
  document.dispatchEvent(new window.CustomEvent("open-settings"));
  await tick();
  await tick();
  return ctx;
}

test("memory rows expose Pin and pinning invokes memory_pin (Task 7.2)", async () => {
  const ctx = await bootWithMemories([
    {
      id: 7,
      kind: "preference",
      content: "Prefers concise answers",
      pinned: false,
    },
  ]);
  const list = document.getElementById("memoryList");
  const pin = list.querySelector("button[data-pin]");
  assert.ok(pin, "pin button present");
  assert.equal(pin.textContent, "Pin");
  assert.equal(pin.dataset.pin, "7");
  pin.click();
  await tick();
  const call = ctx.invokeLog.find((c) => c.name === "memory_pin");
  assert.ok(call, "memory_pin invoked");
  assert.equal(call.payload.id, 7);
  assert.equal(call.payload.pinned, true);
});

test("a pinned memory shows the badge and an Unpin control (Task 7.2)", async () => {
  await bootWithMemories([
    { id: 8, kind: "preference", content: "Uses IFRS", pinned: true },
  ]);
  const list = document.getElementById("memoryList");
  const badge = list.querySelector(".memory-pin-badge");
  assert.ok(badge, "pin badge present");
  assert.equal(badge.getAttribute("aria-label"), "pinned");
  assert.ok(badge.querySelector("svg"), "pin is an svg glyph, not an emoji");
  const pin = list.querySelector("button[data-pin]");
  assert.equal(pin.textContent, "Unpin");
});

test("editing a memory invokes memory_edit with the new text (Task 7.2)", async () => {
  const ctx = await bootWithMemories([
    { id: 5, kind: "preference", content: "old text", pinned: false },
  ]);
  ctx.invokeHandlers.memory_edit = async () => ({ ok: true });
  const list = document.getElementById("memoryList");
  const editBtn = list.querySelector("button[data-edit]");
  assert.ok(editBtn, "Edit button present");
  editBtn.click();
  await tick();
  const input = list.querySelector("input.memory-edit-input");
  assert.ok(input, "inline editor appears");
  assert.equal(input.value, "old text");
  input.value = "new text";
  list.querySelector("button[data-edit-save]").click();
  await tick();
  const call = ctx.invokeLog.find((c) => c.name === "memory_edit");
  assert.ok(call, "memory_edit invoked");
  assert.equal(call.payload.id, 5);
  assert.equal(call.payload.value, "new text");
});

test("memory filter hides non-matching rows (Task 7.2)", async () => {
  await bootWithMemories([
    {
      id: 1,
      kind: "preference",
      content: "prefers DCF valuation",
      pinned: false,
    },
    {
      id: 2,
      kind: "preference",
      content: "uses IFRS standards",
      pinned: false,
    },
  ]);
  const filter = document.getElementById("memoryFilter");
  filter.value = "ifrs";
  filter.dispatchEvent(new window.Event("input"));
  await tick();
  const rows = [...document.querySelectorAll("#memoryList .memory-row")];
  const visible = rows.filter((r) => !r.hidden);
  assert.equal(visible.length, 1, "only the matching row stays visible");
  assert.match(visible[0].dataset.content, /IFRS/);
});
