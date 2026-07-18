import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

async function openWith(readResult, { throwErr = false } = {}) {
  const ctx = setupDom();
  ctx.invokeHandlers.read_page = async () => {
    if (throwErr) throw new Error("boom");
    return readResult;
  };
  const reader = await importModule("reader.mjs");
  reader.initReader();
  await reader.openReader("https://example.com/x", "Example");
  await tick();
  return { reader, body: document.getElementById("readerBody") };
}

test("ok status → ready state, find enabled", async () => {
  const { body } = await openWith({ status: "ok", text: "Hello world ".repeat(40) });
  assert.equal(body.dataset.state, "ready");
  assert.equal(document.getElementById("readerFind").hidden, false);
});

test("blocked status → blocked state, find hidden, open CTA", async () => {
  const { body } = await openWith({ status: "blocked", text: "" });
  assert.equal(body.dataset.state, "blocked");
  assert.equal(document.getElementById("readerFind").hidden, true);
  assert.ok(body.querySelector(".reader-cta"), "open-in-browser CTA present");
});

test("thin status with text → thin state, find enabled", async () => {
  const { body } = await openWith({ status: "thin", text: "short" });
  assert.equal(body.dataset.state, "thin");
  assert.equal(document.getElementById("readerFind").hidden, false);
});

test("ok but empty text → empty state", async () => {
  const { body } = await openWith({ status: "ok", text: "   " });
  assert.equal(body.dataset.state, "empty");
});

test("read_page throws → failed state with retry", async () => {
  const { body } = await openWith(null, { throwErr: true });
  assert.equal(body.dataset.state, "failed");
  assert.ok(body.querySelector("[data-reader-retry]"), "retry offered");
});

test("find with no match → ready-no-match preserves content", async () => {
  const { reader } = await openWith({ status: "ok", text: "alpha beta gamma ".repeat(20) });
  // Drive find directly (debounced path exercised via findNow).
  const body = document.getElementById("readerBody");
  const beforeLen = body.textContent.length;
  // Simulate the debounced find firing with a term absent from the text.
  document.getElementById("readerFind").value = "zzzznotpresent";
  document.getElementById("readerFind").dispatchEvent(new window.Event("input"));
  await new Promise((r) => setTimeout(r, 200));
  assert.equal(body.dataset.state, "ready-no-match");
  assert.ok(body.querySelector(".reader-nomatch"), "no-match banner shown");
  assert.ok(body.textContent.length >= beforeLen, "source content preserved");
});

test("find with a match → ready-highlighted", async () => {
  await openWith({ status: "ok", text: "the quick brown fox ".repeat(20) });
  const body = document.getElementById("readerBody");
  document.getElementById("readerFind").value = "quick";
  document.getElementById("readerFind").dispatchEvent(new window.Event("input"));
  await new Promise((r) => setTimeout(r, 200));
  assert.equal(body.dataset.state, "ready-highlighted");
  assert.ok(body.querySelector("mark.find-hit"), "match highlighted");
});

test("stale response is ignored (newer open wins)", async () => {
  const ctx = setupDom();
  let calls = 0;
  ctx.invokeHandlers.read_page = async (p) => {
    calls += 1;
    const mine = calls;
    // First call resolves LAST (stale); second resolves first.
    await new Promise((r) => setTimeout(r, mine === 1 ? 60 : 5));
    return { status: "ok", text: `body-${p.url}` };
  };
  const reader = await importModule("reader.mjs");
  reader.initReader();
  const p1 = reader.openReader("https://a.com/1", "A");
  const p2 = reader.openReader("https://b.com/2", "B");
  await Promise.all([p1, p2]);
  await new Promise((r) => setTimeout(r, 100));
  const body = document.getElementById("readerBody");
  assert.ok(body.textContent.includes("b.com/2"), "newest wins");
  assert.ok(!body.textContent.includes("a.com/1"), "stale discarded");
});

test("close returns focus to opener and resets dock-open", async () => {
  const ctx = setupDom();
  ctx.invokeHandlers.read_page = async () => ({ status: "ok", text: "x ".repeat(120) });
  const reader = await importModule("reader.mjs");
  reader.initReader();
  // A focusable opener.
  const opener = document.getElementById("chatSend");
  opener.focus();
  await reader.openReader("https://example.com/x", "Example");
  await tick();
  assert.ok(document.body.classList.contains("dock-open"));
  reader.closeReader();
  assert.ok(!document.body.classList.contains("dock-open"));
  assert.equal(document.activeElement, opener, "focus returned to opener");
});
