import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

async function bootSidebar(convos) {
  const ctx = setupDom();
  ctx.invokeHandlers.list_conversations = async () => convos;
  const sb = await importModule("sidebar.mjs");
  sb.initSidebar({ onSelect: () => {}, onNew: () => {} });
  await sb.refresh();
  await tick();
  return { ctx, sb };
}

const CONVOS = [
  { id: "1-aaaa", title: "NVDA analysis", updated: new Date().toISOString() },
  { id: "2-bbbb", title: "AAPL brief", updated: new Date().toISOString() },
];

test("rows are non-interactive containers with a real select button", async () => {
  await bootSidebar(CONVOS);
  const rows = document.querySelectorAll("#convList .conv-row");
  assert.equal(rows.length, 2);
  for (const row of rows) {
    // The row itself is not a button and has no tabindex (no nested-interactive).
    assert.notEqual(row.getAttribute("role"), "button");
    assert.equal(row.hasAttribute("tabindex"), false);
    // Selection is a real <button>.
    const open = row.querySelector("button.conv-open");
    assert.ok(open, "conv-open button present");
  }
});

test("active conversation carries aria-current", async () => {
  const { sb } = await bootSidebar(CONVOS);
  sb.setActive("2-bbbb");
  const active = document.querySelector('.conv-open[aria-current="true"]');
  assert.ok(active, "aria-current set");
  assert.equal(active.closest(".conv-row").dataset.id, "2-bbbb");
});

test("sidebar toggle exposes aria-expanded / controls", async () => {
  await bootSidebar(CONVOS);
  const toggle = document.getElementById("sidebarToggle");
  assert.equal(toggle.getAttribute("aria-controls"), "sidebar");
  assert.equal(toggle.getAttribute("aria-expanded"), "true");
  toggle.click();
  assert.equal(toggle.getAttribute("aria-expanded"), "false", "collapsed → expanded false");
  assert.ok(document.getElementById("sidebar").hasAttribute("inert"), "collapsed sidebar inert");
});

test("rename failure announces + offers retry, retains title", async () => {
  const { ctx } = await bootSidebar(CONVOS);
  ctx.invokeHandlers.rename_conversation = async () => {
    throw new Error("disk full");
  };
  // Enter rename on the first row.
  const row = document.querySelector('.conv-row[data-id="1-aaaa"]');
  row.querySelector(".conv-rename").click();
  const input = row.querySelector(".conv-rename-input");
  assert.ok(input, "rename input shown");
  input.value = "New name";
  input.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter" }));
  await tick();
  await tick();
  const alert = document.getElementById("sidebarAlert");
  assert.equal(alert.hidden, false, "failure announced");
  assert.match(alert.textContent, /disk full/);
  assert.ok(alert.querySelector("button"), "keyboard-reachable action");
});

test("delete confirm then failure announces + retry", async () => {
  const { ctx } = await bootSidebar(CONVOS);
  ctx.invokeHandlers.delete_conversation = async () => {
    throw new Error("locked");
  };
  const row = document.querySelector('.conv-row[data-id="2-bbbb"]');
  row.querySelector(".conv-delete").click();
  await tick();
  const yes = row.querySelector(".conv-del-yes");
  assert.ok(yes, "confirm button shown");
  yes.click();
  await tick();
  await tick();
  const alert = document.getElementById("sidebarAlert");
  assert.equal(alert.hidden, false);
  assert.match(alert.textContent, /locked/);
});
