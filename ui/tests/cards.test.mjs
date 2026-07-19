// cards.test.mjs — card renderer regression (Task 4.2 verification card)

import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule } from "./harness.mjs";

test("renderCard renders a verified verification card", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "verification",
    status: "verified",
    verified: 2,
    total: 2,
    source: "SEC EDGAR XBRL",
  });
  assert.match(el.className, /card-verify/);
  assert.match(el.className, /status-verified/);
  const text = el.textContent;
  assert.match(text, /Verified/);
  assert.match(text, /2\/2/);
  assert.match(text, /SEC EDGAR XBRL/);
});

test("renderCard verification card shows partial-unverified badge", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "verification",
    status: "partial_unverified",
    verified: 1,
    total: 3,
  });
  assert.match(el.className, /status-partial_unverified/);
  assert.match(el.textContent, /Partial/);
  assert.match(el.textContent, /1\/3/);
});

test("renderCard renders a financials card with figures", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "financials",
    ticker: "NVDA",
    entity: "NVIDIA Corp",
    fiscal_year: "2024",
    period_end: "2024-01-28",
    currency: "USD",
    source: "https://www.sec.gov/x",
    rows: [
      { label: "Revenue", value: 60922000000, display: "60,922.0" },
      { label: "Diluted EPS", value: 1.19, display: "1.19" },
    ],
  });
  assert.match(el.className, /card-financials/);
  const text = el.textContent;
  // Entity + fiscal period in the head, and the actual figures in the table.
  assert.match(text, /NVIDIA Corp/);
  assert.match(text, /FY2024/);
  assert.match(text, /Revenue/);
  assert.match(text, /60,922\.0/);
  assert.match(text, /Diluted EPS/);
  assert.match(text, /1\.19/);
  // Not the unknown-card fallback (which would show only the type string).
  assert.doesNotMatch(el.className, /card-unknown/);
});

test("renderCard renders a multi-year financials spread with derived rows", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "financials",
    ticker: "TSLA",
    entity: "Tesla, Inc.",
    currency: "USD",
    periods: [
      { label: "FY2025", end: "2025-12-31" },
      { label: "FY2024", end: "2024-12-31" },
      { label: "FY2023", end: "2023-12-31" },
    ],
    rows: [
      { label: "Revenue", kind: "reported", values: ["$94.83B", "$97.69B", "$96.77B"] },
      { label: "Revenue growth YoY", kind: "derived", values: ["-2.9%", "+1.0%", null] },
    ],
  });
  const heads = [...el.querySelectorAll("thead th")].map((h) => h.textContent);
  assert.deepEqual(heads, ["Line item", "FY2025", "FY2024", "FY2023"]);
  const rows = [...el.querySelectorAll("tbody tr")];
  assert.equal(rows.length, 2);
  // Reported row: three value cells.
  assert.deepEqual(
    [...rows[0].querySelectorAll("td")].map((t) => t.textContent),
    ["Revenue", "$94.83B", "$97.69B", "$96.77B"],
  );
  // Derived row is visually distinguished; a missing value renders an em dash.
  assert.match(rows[1].className, /fin-derived/);
  assert.equal([...rows[1].querySelectorAll("td")].pop().textContent, "—");
});

test("renderCard legacy single-year financials card still renders", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "financials",
    ticker: "NVDA",
    currency: "USD",
    fiscal_year: "2025",
    rows: [{ label: "Revenue", value: 130.5e9, display: "$130.50B" }],
  });
  const heads = [...el.querySelectorAll("thead th")].map((h) => h.textContent);
  assert.deepEqual(heads, ["Line item", "USD"]);
  assert.match(el.textContent, /\$130\.50B/);
});
