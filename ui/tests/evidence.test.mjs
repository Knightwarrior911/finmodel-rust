import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule } from "./harness.mjs";

async function boot() {
  setupDom();
  return importModule("evidence.mjs");
}

const researchCard = {
  type: "research_answer",
  answer: {
    sources: [
      {
        id: "S1",
        final_url: "https://ir.tesla.com/press-release/q1",
        title: "Q1 2026 Update",
        domain: "ir.tesla.com",
        status: "read",
        kind: "company",
      },
      {
        id: "S2",
        final_url: "https://www.reuters.com/tsla-tariffs",
        title: "Tesla flags tariff pressure",
        domain: "reuters.com",
        status: "read",
        kind: "news",
      },
    ],
  },
};

test("sources dedupe by url and keep first-seen numbering", async () => {
  const ev = await boot();
  const st = ev.createEvidenceState();
  assert.ok(ev.evidenceAddCard(st, researchCard));
  // Same source cited again by a later card — no duplicate, no renumber.
  ev.evidenceAddCard(st, {
    type: "deal",
    sources_read: ["https://ir.tesla.com/press-release/q1"],
  });
  assert.equal(st.sources.length, 2);
  assert.ok(st.sources[0].url.includes("ir.tesla.com"));
  const el = document.createElement("div");
  ev.renderEvidenceSources(el, st, {});
  assert.match(el.textContent, /Sources · 2/);
  assert.match(el.textContent, /Q1 2026 Update/);
  // Numbered like inline cites.
  const nums = [...el.querySelectorAll(".src-card-num")].map((n) => n.textContent);
  assert.deepEqual(nums, ["1", "2"]);
});

test("filing and page cards land in the source ledger", async () => {
  const ev = await boot();
  const st = ev.createEvidenceState();
  ev.evidenceAddCard(st, {
    type: "filing_doc",
    ticker: "TSLA",
    form: "10-Q",
    filing_date: "2026-04-22",
    url: "https://www.sec.gov/Archives/tsla-10q.htm",
  });
  ev.evidenceAddCard(st, {
    type: "page",
    url: "https://www.tesla.com/newsroom/q1",
    title: "Tesla newsroom",
  });
  assert.equal(st.sources.length, 2);
  assert.equal(st.sources[0].kind, "filing");
});

test("artifacts collect newest-first and rebuilds float to the top", async () => {
  const ev = await boot();
  const st = ev.createEvidenceState();
  ev.evidenceAddCard(st, {
    type: "model",
    ticker: "NVDA",
    xlsx_path: "C:/out/nvda.xlsx",
  });
  ev.evidenceAddCard(st, {
    type: "model",
    ticker: "TSLA",
    xlsx_path: "C:/out/tsla.xlsx",
    pptx_path: "C:/out/tsla.pptx",
  });
  assert.equal(st.artifacts.length, 3);
  assert.equal(st.artifacts[0].kind, "deck"); // TSLA deck added last → first
  // NVDA rebuilt → floats back to the top without duplicating.
  ev.evidenceAddCard(st, {
    type: "model",
    ticker: "NVDA",
    xlsx_path: "C:/out/nvda.xlsx",
  });
  assert.equal(st.artifacts.length, 3);
  assert.equal(st.artifacts[0].label, "NVDA model");
  const el = document.createElement("div");
  let opened = null;
  ev.renderEvidenceArtifacts(el, st, { onOpen: (p) => (opened = p) });
  assert.match(el.textContent, /Artifacts · 3/);
  el.querySelector("[data-art-path]").click();
  assert.equal(opened, "C:/out/nvda.xlsx");
});

test("valuation keeps the latest snapshot per ticker plus verification", async () => {
  const ev = await boot();
  const st = ev.createEvidenceState();
  ev.evidenceAddCard(st, {
    type: "model",
    ticker: "TSLA",
    currency: "USD",
    valuation: { has_dcf: true, price_per_share: 212.5, current_price: 240, upside_pct: -11.5, ev: 780000, wacc: 9.1 },
  });
  // A rebuilt model replaces the old snapshot for the same ticker.
  ev.evidenceAddCard(st, {
    type: "model",
    ticker: "TSLA",
    currency: "USD",
    valuation: { has_dcf: true, price_per_share: 230, current_price: 240, upside_pct: -4.2, ev: 800000, wacc: 9.1 },
  });
  ev.evidenceAddCard(st, {
    type: "verification",
    status: "verified",
    verified: 6,
    total: 6,
  });
  assert.equal(st.valuations.size, 1);
  const el = document.createElement("div");
  ev.renderEvidenceValuation(el, st);
  assert.match(el.textContent, /TSLA · USD/);
  assert.match(el.textContent, /230/);
  assert.doesNotMatch(el.textContent, /212\.5/);
  assert.match(el.textContent, /6\/6 figures/);
});

test("empty panels keep their warm empty-state copy", async () => {
  const ev = await boot();
  const st = ev.createEvidenceState();
  const s = document.createElement("div");
  const v = document.createElement("div");
  const a = document.createElement("div");
  ev.renderEvidenceSources(s, st, {});
  ev.renderEvidenceValuation(v, st);
  ev.renderEvidenceArtifacts(a, st, {});
  assert.match(s.textContent, /Sources I cite will collect here/);
  assert.match(v.textContent, /Valuation summaries will land here/);
  assert.match(a.textContent, /Workbooks and decks we create/);
});

test("source click opens through the handler with url and title", async () => {
  const ev = await boot();
  const st = ev.createEvidenceState();
  ev.evidenceAddCard(st, researchCard);
  const el = document.createElement("div");
  let got = null;
  ev.renderEvidenceSources(el, st, { onOpen: (url, title) => (got = { url, title }) });
  el.querySelector("[data-src-url]").click();
  assert.ok(got);
  assert.match(got.url, /ir\.tesla\.com/);
  assert.equal(got.title, "Q1 2026 Update");
});
