import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule } from "./harness.mjs";

async function applyWith(settings, theme = "light") {
  setupDom({ theme });
  const chat = await importModule("chat.mjs");
  chat.applyCapability(settings);
  return {
    note: document.getElementById("capabilityNote").textContent,
    chips: [...document.getElementById("exampleChips").querySelectorAll(".example-chip")].map(
      (b) => b.textContent
    ),
  };
}

test("no key → offline demo note + real embedded demo tickers", async () => {
  const { note, chips } = await applyWith({ has_key: false, model: "m", mcp_command: "" });
  assert.match(note, /No LLM key/);
  assert.match(note, /offline demo/i);
  // Demo chips use the actually-embedded fixtures, not live US tickers.
  assert.ok(chips.some((c) => c.includes("SAND.ST")), `demo chips: ${chips.join(" | ")}`);
  assert.ok(!chips.some((c) => c.includes("NVDA")), "no live-only tickers offline");
});

test("key present, probe unknown → run Test model action", async () => {
  const { note, chips } = await applyWith({ has_key: true, model: "m", mcp_command: "x" });
  assert.match(note, /untested/i);
  assert.match(note, /Test model/);
  assert.ok(chips.some((c) => c.includes("NVDA")), "live chips when keyed");
});

test("native + strict verified → verified note", async () => {
  const { note } = await applyWith({
    has_key: true,
    model: "m",
    mcp_command: "x",
    model_capability: { model_id: "m", native_tools: true, strict_json: true },
  });
  assert.match(note, /verified/i);
  assert.match(note, /strict JSON/i);
});

test("text-only model → app-controlled synthesis note", async () => {
  const { note } = await applyWith({
    has_key: true,
    model: "m",
    mcp_command: "x",
    model_capability: { model_id: "m", native_tools: false, strict_json: false },
  });
  assert.match(note, /no verified tool-calling/i);
});

test("no browser configured → basic HTTP reading note", async () => {
  const { note } = await applyWith({ has_key: true, model: "m", mcp_command: "" });
  assert.match(note, /basic HTTP/i);
  assert.match(note, /Roam browser/i);
});

test("capability applies identically in dark theme", async () => {
  const { note } = await applyWith({ has_key: false, model: "m", mcp_command: "" }, "dark");
  assert.match(note, /No LLM key/);
  assert.equal(document.documentElement.dataset.theme, "dark");
});
