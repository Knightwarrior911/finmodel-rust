// analyst.test.mjs — Analyst tools modal regression (Phase 6.5). Drives the real
// module against a mocked Tauri bridge: open/tab/close, missing-equity rejection,
// and each form's invoke payload + rendered output.

import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

async function bootAnalyst() {
  const ctx = setupDom();
  const analyst = await importModule("analyst.mjs");
  analyst.initAnalyst();
  analyst.openAnalyst();
  await tick();
  return { ctx, analyst };
}

function setVal(name, v) {
  const el = document.querySelector(`[name="${name}"]`);
  el.value = String(v);
}

function submitForm(id) {
  const f = document.getElementById(id);
  f.dispatchEvent(new window.Event("submit", { bubbles: true, cancelable: true }));
}

test("opening analyst shows modal, traps focus, and tabs switch panels", async () => {
  await bootAnalyst();
  const modal = document.getElementById("analystModal");
  assert.equal(modal.hidden, false, "modal shown");
  assert.ok(document.getElementById("app").hasAttribute("inert"), "app inert behind dialog");
  // Default panel is EV; switching tab reveals IFRS and hides EV.
  assert.equal(document.getElementById("evForm").hidden, false);
  document.querySelector('.analyst-tab[data-tab="ifrs"]').click();
  assert.equal(document.getElementById("evForm").hidden, true, "EV hidden after tab switch");
  assert.equal(document.getElementById("ifrsForm").hidden, false, "IFRS shown");
});

test("EV bridge rejects missing equity value without invoking", async () => {
  const { ctx } = await bootAnalyst();
  ctx.invokeHandlers.ev_bridge = async () => ({ total_ev: 0, additions: [], subtractions: [] });
  setVal("total_debt", 200);
  submitForm("evForm");
  await tick();
  const status = document.getElementById("analystStatus");
  assert.equal(status.hidden, false);
  assert.match(status.textContent, /market cap/i, "error asks for equity value");
  assert.equal(
    ctx.invokeLog.filter((c) => c.name === "ev_bridge").length,
    0,
    "no invoke when equity missing"
  );
});

test("EV bridge submits input and renders enterprise value", async () => {
  const { ctx } = await bootAnalyst();
  ctx.invokeHandlers.ev_bridge = async () => ({
    market_cap: 1000,
    additions: [{ item: "Total debt", amount: 200, source: "BS" }],
    subtractions: [{ item: "Cash", amount: 100, source: "BS" }],
    total_ev: 1100,
  });
  setVal("market_cap", 1000);
  setVal("total_debt", 200);
  setVal("cash", 100);
  submitForm("evForm");
  await tick();
  await tick();
  const call = ctx.invokeLog.find((c) => c.name === "ev_bridge");
  assert.ok(call, "ev_bridge invoked");
  assert.equal(call.payload.input.market_cap, 1000, "market cap forwarded");
  assert.equal(call.payload.input.total_debt, 200, "debt forwarded");
  const result = document.getElementById("analystResult").innerHTML;
  assert.match(result, /Enterprise value/i);
  assert.match(result, /1,100M/);
});

test("IFRS bridge forwards input + revenue and renders adjusted metrics", async () => {
  const { ctx } = await bootAnalyst();
  ctx.invokeHandlers.ifrs_bridge = async () => ({
    direction: "UsGaapToIfrs",
    adjusted_ebit: 580,
    adjusted_ebitda: 600,
    adjusted_ebita: 590,
    reported_ebit_margin: 50,
    adjusted_ebit_margin: 58,
    reported_ebitda_margin: 60,
    adjusted_ebitda_margin: 60,
    reported_ebita_margin: 55,
    adjusted_ebita_margin: 59,
    ebit_delta: 80,
    ebitda_delta: 0,
    ebita_delta: 40,
  });
  document.querySelector('.analyst-tab[data-tab="ifrs"]').click();
  for (const [k, v] of [
    ["revenue", 1000],
    ["rou_depreciation", 80],
    ["lease_interest", 20],
    ["short_term_rent", 0],
    ["reported_ebit", 500],
    ["reported_ebitda", 600],
    ["reported_ebita", 550],
    ["standard_depreciation", 0],
    ["standard_amortization", 0],
  ]) {
    setVal(k, v);
  }
  submitForm("ifrsForm");
  await tick();
  await tick();
  const call = ctx.invokeLog.find((c) => c.name === "ifrs_bridge");
  assert.ok(call, "ifrs_bridge invoked");
  assert.equal(call.payload.revenue, 1000, "revenue split out");
  assert.equal(call.payload.input.reported_ebit, 500, "reported EBIT forwarded");
  assert.equal(call.payload.input.revenue, undefined, "revenue not left in input");
  const result = document.getElementById("analystResult").innerHTML;
  assert.match(result, /Adjusted/);
  assert.match(result, /580M/, "adjusted EBIT rendered");
});

test("tie-out forwards both JSON documents and renders the score", async () => {
  const { ctx } = await bootAnalyst();
  ctx.invokeHandlers.tie_out = async () => ({
    trusted: 10,
    matched: 9,
    percentage: 90,
    per_statement: {},
    mismatches: [{ statement: "IS", key: "revenue", year: 2023, ground_truth: 1, model: 2 }],
  });
  document.querySelector('.analyst-tab[data-tab="tieout"]').click();
  document.querySelector('[name="ground_truth_json"]').value = '{"gt":1}';
  document.querySelector('[name="model_json"]').value = '{"m":2}';
  submitForm("tieoutForm");
  await tick();
  await tick();
  const call = ctx.invokeLog.find((c) => c.name === "tie_out");
  assert.ok(call, "tie_out invoked");
  assert.equal(call.payload.ground_truth_json, '{"gt":1}');
  assert.equal(call.payload.model_json, '{"m":2}');
  const result = document.getElementById("analystResult").innerHTML;
  assert.match(result, /9 \/ 10/);
  assert.match(result, /90\.0%/);
  assert.match(result, /revenue/);
});

test("Escape closes analyst and clears inert", async () => {
  await bootAnalyst();
  const card = document.getElementById("analystModal").querySelector(".modal-card");
  card.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await tick();
  assert.equal(document.getElementById("analystModal").hidden, true, "closed on Escape");
  assert.ok(!document.getElementById("app").hasAttribute("inert"), "inert cleared");
});
