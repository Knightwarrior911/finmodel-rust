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
  assert.match(text, /Figures checked|Verified/);
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
  assert.match(el.textContent, /Partly checked|Partial/);
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

test("research answer card uses warm copy, not schema voice", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "research_answer",
    answer: {
      confidence: "medium",
      summary: {
        text: "Revenue grew on data-center demand.",
        citations: [{ source_id: "s1", quote: "Data center revenue rose." }],
      },
      sections: [],
      sources: [
        {
          id: "s1",
          domain: "sec.gov",
          status: "ok",
          kind: "filing",
          final_url: "https://www.sec.gov/a",
        },
      ],
      limitations: ["Guidance not yet confirmed."],
    },
  });
  const text = el.textContent;
  assert.match(text, /Research notes/);
  assert.match(text, /Fairly confident/);
  assert.doesNotMatch(text, /confidence:/i);
  assert.match(text, /Sources · 1/);
  assert.match(text, /Worth keeping in mind/);
  assert.match(text, /Read · Filing/);
  assert.doesNotMatch(text, /\[s1\]/);
  const cite = el.querySelector(".cite-ref");
  assert.ok(cite);
  assert.equal(cite.textContent, "1");
  const card = el.querySelector(".src-card");
  assert.ok(card);
  assert.equal(card.querySelector(".src-card-num").textContent, "1");
  assert.match(card.querySelector(".src-card-avatar").textContent, /S/i);
  assert.equal(card.getAttribute("data-url"), "https://www.sec.gov/a");
});

test("deal card never dumps JSON or raw sufficiency enums", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "deal",
    acquirer: "Alpha",
    target: "Beta",
    sufficient: false,
    summary: {
      deal_value: { amount: "12", currency: "USD bn" },
      announce_date: "2024-01-15",
    },
    sources_read: ["https://www.example.com/deal"],
  });
  const text = el.textContent;
  assert.match(text, /Still missing pieces/);
  assert.doesNotMatch(text, /\bsufficient\b|\bpartial\b/);
  assert.doesNotMatch(text, /\{|\}/);
  assert.match(text, /Deal value/);
  assert.match(text, /12 USD bn/);
  assert.match(text, /Announced/);
  assert.match(text, /Sources/);
});

test("quote and page cards speak plainly", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const quote = cards.renderCard({
    type: "quote",
    ticker: "NVDA",
    price: 120.5,
    currency: "USD",
    week52_low: 90,
    week52_high: 140,
  });
  assert.match(quote.textContent, /Last price/);
  assert.match(quote.textContent, /52-week/);
  assert.doesNotMatch(quote.textContent, /\b52w\b/);

  const page = cards.renderCard({
    type: "page",
    title: "NVIDIA IR",
    url: "https://investor.nvidia.com/overview",
    status: "ok",
  });
  assert.match(page.textContent, /Ready to read/);
  assert.doesNotMatch(page.textContent, / · ok/);
});

test("verification card uses colleague language", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "verification",
    status: "partial_unverified",
    verified: 1,
    total: 3,
    source: "SEC EDGAR XBRL",
  });
  assert.match(el.textContent, /Partly checked/);
  assert.match(el.textContent, /key figures checked/);
  assert.doesNotMatch(el.textContent, /Partial — unverified|material figures verified/);
});
