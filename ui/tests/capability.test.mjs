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

test("no key → friendly demo-mode note + real embedded demo tickers", async () => {
  const { note, chips } = await applyWith({ has_key: false, model: "m", mcp_command: "" });
  assert.match(note, /demo mode/i);
  assert.match(note, /Settings/);
  // No developer jargon in the consumer greeting.
  assert.doesNotMatch(note, /tool-calling|strict JSON|basic HTTP|Roam/i);
  // Demo chips use the actually-embedded fixtures, not live US tickers.
  assert.ok(chips.some((c) => c.includes("SAND.ST")), `demo chips: ${chips.join(" | ")}`);
  assert.ok(!chips.some((c) => c.includes("NVDA")), "no live-only tickers offline");
});

test("key present, capability unknown → ready + live chips", async () => {
  const { note, chips } = await applyWith({ has_key: true, model: "m", mcp_command: "x" });
  assert.match(note, /Ready to analyze/i);
  assert.doesNotMatch(note, /tool-calling|strict JSON|basic HTTP|Roam|untested/i);
  assert.ok(chips.some((c) => c.includes("NVDA")), "live chips when keyed");
});

test("tool-capable model → ready note (no jargon)", async () => {
  const { note } = await applyWith({
    has_key: true,
    model: "m",
    mcp_command: "x",
    model_capability: { model_id: "m", native_tools: true, strict_json: true },
  });
  assert.match(note, /Ready to analyze/i);
  assert.doesNotMatch(note, /tool-calling|strict JSON/i);
});

test("model can't use tools → plain-language limitation + fix", async () => {
  const { note } = await applyWith({
    has_key: true,
    model: "m",
    mcp_command: "x",
    model_capability: { model_id: "m", native_tools: false, strict_json: false },
  });
  assert.match(note, /can't pull live data|can't .* build models/i);
  assert.match(note, /Settings/);
  assert.doesNotMatch(note, /tool-calling|strict JSON/i);
});

test("capability applies identically in dark theme", async () => {
  const { note } = await applyWith({ has_key: false, model: "m", mcp_command: "" }, "dark");
  assert.match(note, /demo mode/i);
  assert.equal(document.documentElement.dataset.theme, "dark");
});
