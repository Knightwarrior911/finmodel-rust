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
