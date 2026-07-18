// workbench.test.mjs — Evidence dock regression (Task 2.2). Drives the real
// module against the shipped index.html DOM: open/close/toggle, tab selection,
// the Ctrl/⌘ keyboard map, roving tablist arrow nav, and focus return.

import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

async function bootWorkbench() {
  const ctx = setupDom();
  const wb = await importModule("workbench.mjs");
  wb.initWorkbench();
  return { ctx, wb };
}

function keydown(opts) {
  document.dispatchEvent(new window.KeyboardEvent("keydown", { bubbles: true, ...opts }));
}

test("openDock shows the dock, applies body.dock-open, and selects the tab", async () => {
  const { wb } = await bootWorkbench();
  wb.openDock("valuation");
  assert.ok(document.body.classList.contains("dock-open"), "dock-open class");
  assert.equal(document.getElementById("evidenceDock").hidden, false, "dock visible");
  assert.equal(wb.activeDockTab(), "valuation");
  assert.equal(
    document.getElementById("dockTab-valuation").getAttribute("aria-selected"),
    "true"
  );
  assert.equal(document.getElementById("dockPanel-valuation").hidden, false);
  assert.equal(document.getElementById("dockPanel-model").hidden, true);
});

test("Ctrl+3 opens the dock on the Sources tab", async () => {
  const { wb } = await bootWorkbench();
  keydown({ key: "3", ctrlKey: true });
  assert.ok(wb.isDockOpen(), "dock opened by shortcut");
  assert.equal(wb.activeDockTab(), "sources");
});

test("Ctrl+J toggles the dock open then closed", async () => {
  const { wb } = await bootWorkbench();
  keydown({ key: "j", ctrlKey: true });
  assert.ok(wb.isDockOpen(), "opened");
  keydown({ key: "j", ctrlKey: true });
  assert.ok(!wb.isDockOpen(), "closed");
});

test("roving tablist arrow keys move the selected tab", async () => {
  const { wb } = await bootWorkbench();
  wb.openDock("model");
  const tablist = document.querySelector(".dock-tabs");
  tablist.dispatchEvent(
    new window.KeyboardEvent("keydown", { key: "ArrowRight", bubbles: true })
  );
  assert.equal(wb.activeDockTab(), "valuation", "→ advances one tab");
  tablist.dispatchEvent(
    new window.KeyboardEvent("keydown", { key: "End", bubbles: true })
  );
  assert.equal(wb.activeDockTab(), "reader", "End jumps to last");
  tablist.dispatchEvent(
    new window.KeyboardEvent("keydown", { key: "ArrowLeft", bubbles: true })
  );
  assert.equal(wb.activeDockTab(), "artifacts", "← wraps/moves back");
});

test("only the active tab is in the tab order (roving tabindex)", async () => {
  const { wb } = await bootWorkbench();
  wb.openDock("sources");
  assert.equal(document.getElementById("dockTab-sources").tabIndex, 0);
  assert.equal(document.getElementById("dockTab-model").tabIndex, -1);
  assert.equal(document.getElementById("dockTab-reader").tabIndex, -1);
});

test("closeDock returns focus to the invoker", async () => {
  const { wb } = await bootWorkbench();
  const opener = document.getElementById("newChatBtn");
  opener.focus();
  wb.openDock("model");
  assert.notEqual(document.activeElement, opener, "focus moved into dock");
  wb.closeDock();
  assert.equal(document.activeElement, opener, "focus returned to opener");
  assert.ok(!document.body.classList.contains("dock-open"));
});

test("Esc closes the dock only when a run is not active", async () => {
  const { wb } = await bootWorkbench();
  wb.openDock("model");
  document.getElementById("dockTab-model").focus();
  // Simulate an active run: Stop button visible → Esc must NOT close the dock.
  const stop = document.getElementById("chatStop");
  stop.hidden = false;
  keydown({ key: "Escape" });
  assert.ok(wb.isDockOpen(), "dock stays open while a run is active");
  // Run ends → Esc closes the dock.
  stop.hidden = true;
  document.getElementById("dockTab-model").focus();
  keydown({ key: "Escape" });
  assert.ok(!wb.isDockOpen(), "dock closes when idle");
});
