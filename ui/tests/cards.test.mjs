// cards.test.mjs — card renderer regression (Task 4.2 verification card)

import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

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

test("filing_doc: section read shows its human name and preview, no byte counts", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "filing_doc",
    ticker: "TSLA",
    form: "8-K",
    filing_date: "2026-07-02",
    url: "https://www.sec.gov/x",
    item: "2",
    items: ["2", "9"],
    chars: 574,
    preview:
      "Tesla's revenue for the quarter reflects tariff-related cost pressure and…",
  });
  const text = el.textContent;
  assert.ok(
    text.includes("Item 2 · Financial information"),
    "section read is named in plain English",
  );
  assert.ok(
    text.includes("tariff-related cost pressure"),
    "the actual excerpt opening is quoted",
  );
  assert.ok(!text.includes("characters"), "no byte-count schema-speak");
  assert.ok(!text.includes("574"), "char count is gone");
  assert.ok(
    !el.querySelector(".filing-item-chip"),
    "a section read does not also wear the whole-document chip wall",
  );
  assert.ok(
    text.includes("Current report"),
    "form code carries its plain name",
  );
});

test("filing_doc: whole-document open lists named contents", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "filing_doc",
    ticker: "TSLA",
    form: "8-K",
    filing_date: "2026-07-02",
    url: "https://www.sec.gov/x",
    item: null,
    items: ["2", "9"],
  });
  const chips = [...el.querySelectorAll(".filing-item-chip")].map(
    (c) => c.textContent,
  );
  assert.deepEqual(chips, [
    "Item 2 · Financial information",
    "Item 9 · Financial statements and exhibits",
  ]);
});

test("financials card: basis chips render and swap the card in place", async () => {
  const ctx = setupDom();
  const cards = await importModule("cards.mjs");
  const host = document.createElement("div");
  document.body.appendChild(host);
  let asked = null;
  ctx.invokeHandlers.financials_card = async (args) => {
    asked = args;
    return { type: "financials", ticker: "TSLA", basis: "quarterly", rows: [{ label: "Revenue", display: "25,000" }] };
  };
  const el = cards.renderCard({
    type: "financials", ticker: "TSLA", basis: "annual",
    rows: [{ label: "Revenue", display: "97,690" }],
  });
  host.appendChild(el);
  const chips = [...el.querySelectorAll(".basis-chip")].map((c) => c.textContent);
  assert.deepEqual(chips, ["Annual", "Quarterly", "LTM", "Half-year"]);
  assert.ok(el.querySelector('.basis-chip[data-basis="annual"]').classList.contains("active"));
  el.querySelector('[data-basis="quarterly"]').click();
  await tick();
  await tick();
  assert.deepEqual(asked, { ticker: "TSLA", basis: "quarterly" });
  const swapped = host.querySelector(".card-financials");
  assert.match(swapped.textContent, /25,000/);
  assert.ok(swapped.querySelector('.basis-chip[data-basis="quarterly"]').classList.contains("active"));
});

test("financials card renders the segment revenue section with eliminations labeled", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "financials", ticker: "TSLA", basis: "annual", currency: "USD",
    rows: [{ label: "Revenue", display: "97,690" }],
    segments: [
      { segment: "Automotive", member: "tsla:AutomotiveSegmentMember", value: 82056000000, period_end: "2025-12-31", eliminations: false },
      { segment: "Energy Generation And Storage", member: "tsla:EnergySegmentMember", value: 12771000000, period_end: "2025-12-31", eliminations: false },
      { segment: "Intersegment Elimination", member: "us-gaap:IntersegmentEliminationMember", value: -500000000, period_end: "2025-12-31", eliminations: true },
    ],
  });
  const seg = el.querySelector(".fin-segments");
  assert.ok(seg, "segments section rendered");
  assert.match(seg.textContent, /Segment revenue · USD millions/);
  assert.match(seg.textContent, /Automotive/);
  assert.match(seg.textContent, /82,056/);
  assert.match(seg.textContent, /Intersegment Elimination \(eliminations\)/);
  // No segments -> no section.
  const plain = cards.renderCard({ type: "financials", ticker: "AAPL", rows: [] });
  assert.ok(!plain.querySelector(".fin-segments"));
});

test("memo card renders kind, sections, sources, and validation note", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "memo", kind: "earnings_note", company: "Tesla, Inc.",
    memo_path: "C:/out/Tesla_earnings_note_2026-07-19.md",
    sections: 3, fallback_sections: 1, sources: 4,
  });
  const text = el.textContent;
  assert.match(text, /Tesla, Inc./);
  assert.match(text, /Earnings note · 3 sections · 4 sources/);
  assert.match(text, /1 section composed directly from the evidence/);
  assert.ok(el.querySelector('[data-open-excel]'), "open action present");
  // Clean draft: no validation note.
  const clean = cards.renderCard({
    type: "memo", kind: "deal_summary", company: "Magna",
    memo_path: "C:/out/m.md", sections: 3, fallback_sections: 0, sources: 2,
  });
  assert.doesNotMatch(clean.textContent, /composed directly/);
});

test("memo card offers the deck when a pptx was produced", async () => {
  setupDom();
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "memo", kind: "comps_note", company: "NVDA",
    sections: 3, sources: 2, memo_path: "C:/x/n.md", pptx_path: "C:/x/n.pptx",
  });
  const html = el.innerHTML;
  assert.match(html, /Open deck/);
  assert.match(html, /n.pptx/);
  const plain = cards.renderCard({
    type: "memo", kind: "earnings_note", company: "TSLA",
    sections: 3, sources: 1, memo_path: "C:/x/t.md",
  });
  assert.ok(!/Open deck/.test(plain.innerHTML), "no deck button without pptx");
});

test('advisor card renders the second-look notes', async () => {
  const cards = await importModule('cards.mjs');
  const el = cards.renderCard({
    type: 'advisor',
    notes: ['Revenue figure not in the evidence', 'Margin overclaim'],
  });
  const html = el.innerHTML;
  assert.match(html, /Second look/);
  assert.match(html, /Revenue figure not in the evidence/);
  assert.equal(el.querySelectorAll('.advisor-notes li').length, 2);
  // Empty notes render nothing (never an empty shell).
  assert.equal(cards.renderCard({ type: 'advisor', notes: [] }).nodeType, 8, 'declines as comment node');
});

test('delegate card shows task, findings, and tools used', async () => {
  const cards = await importModule('cards.mjs');
  const el = cards.renderCard({
    type: 'delegate',
    task: 'Pull FY2025 revenue and operating margin for SAP',
    findings: "SAP FY2025 revenue was EUR 36.8B.\n\nOperating margin held at 26%.",
    tools_used: ['get_financials', 'research'],
  });
  const html = el.innerHTML;
  assert.match(html, /Deep dive/);
  assert.match(html, /SAP FY2025 revenue/);
  assert.match(html, /Checked with: get_financials, research/);
  // No findings -> no card.
  assert.equal(cards.renderCard({ type: 'delegate', task: 'x', findings: '' }).nodeType, 8, 'declines as comment node');
});

test('self-check card carries the drift note', async () => {
  const cards = await importModule('cards.mjs');
  const el = cards.renderCard({ type: 'self_check', message: 'Caught figures with no tool behind them - checking properly.' });
  assert.match(el.innerHTML, /Caught figures/);
});

test("delegate card folds the junior analyst's work trail", async () => {
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "delegate",
    task: "SAP margins",
    findings: "Operating margin held at 26%.",
    tools_used: ["get_financials"],
    trail: [
      { tool: "get_financials", subject: "SAP", note: "SAP SE — annual report" },
      { tool: "research", subject: "SAP margin drivers", note: "3 sources" },
    ],
  });
  const html = el.innerHTML;
  assert.match(html, /How this was worked \(2 checks\)/);
  assert.equal(el.querySelectorAll(".delegate-trail li").length, 2);
  assert.match(html, /SAP SE — annual report/);
  // Trail is collapsed by default (details without open attribute).
  const details = el.querySelector("details.delegate-trail");
  assert.ok(details && !details.hasAttribute("open"));
});

test("turn cost renders tokens always, dollars only when billed", async () => {
  const cards = await importModule("cards.mjs");
  const paid = cards.renderCard({
    type: "turn_cost", prompt_tokens: 11200, completion_tokens: 1300, usd: 0.031,
  });
  assert.match(paid.innerHTML, /This turn: 12.5k tokens · about \$0.031/);
  const free = cards.renderCard({
    type: "turn_cost", prompt_tokens: 400, completion_tokens: 100,
  });
  assert.match(free.innerHTML, /This turn: 500 tokens/);
  assert.ok(!/\$/.test(free.innerHTML), "no fabricated dollars");
  // Zero tokens declines (comment node).
  assert.equal(cards.renderCard({ type: "turn_cost" }).nodeType, 8);
});

test("deepSourceUrl builds Chrome text-fragment links with clean quotes", async () => {
  setupDom();
  const core = await importModule("core.mjs");
  // Plain quote → #:~:text= with encoding; '-' is directive syntax.
  assert.equal(
    core.deepSourceUrl("https://ex.com/a", "revenue rose 8%"),
    "https://ex.com/a#:~:text=revenue%20rose%208%25",
  );
  assert.ok(
    core.deepSourceUrl("https://ex.com/a", "год-over-год").includes("%2D"),
    "dashes are percent-encoded",
  );
  // Search-snippet hygiene: anchor on the LONGEST run between ellipses.
  assert.equal(
    core.fragmentQuote("intro … the exact passage we want to anchor on … tail"),
    "the exact passage we want to anchor on",
  );
  // 10-word cap.
  assert.equal(
    core.fragmentQuote("one two three four five six seven eight nine ten eleven twelve"),
    "one two three four five six seven eight nine ten",
  );
  // Existing fragment → the directive appends after it.
  assert.equal(
    core.deepSourceUrl("https://ex.com/a#sec2", "net income"),
    "https://ex.com/a#sec2:~:text=net%20income",
  );
  // Junk in, plain URL out — never a broken directive, never non-http.
  assert.equal(core.deepSourceUrl("https://ex.com/a", "  …  "), "https://ex.com/a");
  assert.equal(core.deepSourceUrl("javascript:alert(1)", "x"), "");
});

test("financials columns click through to the exact SEC filing", async () => {
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "financials",
    entity: "TestCo",
    currency: "USD",
    periods: [{ label: "FY2024", end: "2024-12-31" }, { label: "FY2023", end: "2023-12-31" }],
    filings: ["https://www.sec.gov/Archives/edgar/data/1/x/a-index.htm", null],
    rows: [{ label: "Revenue", values: ["101,000", "80,000"] }],
  });
  const html = el.innerHTML;
  // FY2024 header + its value are clickable; FY2023 (no filing) is not.
  const links = el.querySelectorAll(".num-link");
  assert.equal(links.length, 2, "header + one value cell");
  for (const l of links) {
    assert.equal(l.dataset.url, "https://www.sec.gov/Archives/edgar/data/1/x/a-index.htm");
  }
  assert.match(html, /opens it on SEC EDGAR/);
  // The un-linked year renders as plain text.
  assert.match(html, /FY2023<\/th>/);
});

test("cite pills deep-link to the quoted passage", async () => {
  const cards = await importModule("cards.mjs");
  const el = cards.renderCard({
    type: "research_answer",
    answer: {
      confidence: "high",
      summary: { text: "Revenue rose.", citations: [{ source_id: "s1", quote: "revenue rose 8% to $4.2 billion" }] },
      sections: [],
      sources: [{ id: "s1", final_url: "https://reuters.com/x", domain: "reuters.com", status: "read", snippet: "revenue rose 8% to $4.2 billion in the quarter" }],
      limitations: [],
    },
  });
  const pill = el.querySelector(".cite-ref");
  assert.ok(pill, "cite pill renders");
  assert.ok(
    pill.dataset.url.startsWith("https://reuters.com/x#:~:text="),
    `deep link built: ${pill.dataset.url}`,
  );
  assert.match(pill.title, /Opens the source at:/);
  // The source card anchors on its own snippet.
  const srcCard = el.querySelector(".src-card");
  assert.ok(srcCard.dataset.url.includes("#:~:text="), "src card deep links");
});
