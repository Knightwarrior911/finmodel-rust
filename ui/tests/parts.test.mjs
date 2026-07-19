// parts.test.mjs — ordered structured-parts renderer tests (Phase D).

import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule } from "./harness.mjs";

async function boot() {
  setupDom();
  const parts = await importModule("parts.mjs");
  const container = document.createElement("div");
  return { parts, container };
}

test("renders parts in authoritative backend order", async () => {
  const { parts, container } = await boot();
  const n = parts.renderParts(container, [
    { kind: "text", payload: { text: "Analysis." } },
    { kind: "sources", payload: { sources: [{ canonical_uri: "https://sec.gov/x", title: "10-K" }] } },
    { kind: "artifact", payload: { id: "art-1", label: "Model", version: 2 } },
  ]);
  assert.equal(n, 3);
  assert.equal(container.children.length, 3);
  assert.ok(container.children[0].classList.contains("part-text"));
  assert.ok(container.children[1].classList.contains("part-sources"));
  assert.ok(container.children[2].classList.contains("part-artifact"));
});

test("text part keeps model text inert (textContent)", async () => {
  const { parts, container } = await boot();
  parts.renderParts(container, [{ kind: "text", payload: { text: "<img src=x onerror=1>" } }]);
  const t = container.querySelector(".part-text");
  assert.equal(t.textContent, "<img src=x onerror=1>");
  assert.equal(t.querySelector("img"), null); // never parsed as HTML
});

test("sources render numbered rows with http link + domain", async () => {
  const { parts, container } = await boot();
  parts.renderParts(container, [
    {
      kind: "sources",
      payload: {
        sources: [
          { id: "s1", canonical_uri: "https://www.sec.gov/a", title: "Filing", published_at: "2024-02-21" },
          { id: "s2", canonical_uri: "https://example.com/b", title: "News" },
        ],
      },
    },
  ]);
  const rows = container.querySelectorAll(".part-source");
  assert.equal(rows.length, 2);
  assert.equal(rows[0].querySelector(".part-source-ref").textContent, "1");
  const link = rows[0].querySelector("a.part-source-link");
  assert.equal(link.getAttribute("href"), "https://www.sec.gov/a");
  assert.equal(rows[0].querySelector(".part-source-domain").textContent, "sec.gov");
  assert.equal(rows[0].querySelector(".part-source-date").textContent, "2024-02-21");
});

test("non-http source shows title, never a live link", async () => {
  const { parts, container } = await boot();
  parts.renderParts(container, [
    { kind: "sources", payload: { sources: [{ title: "Local PDF", canonical_uri: "file:///c/x.pdf" }] } },
  ]);
  assert.equal(container.querySelector("a.part-source-link"), null);
  assert.equal(container.querySelector(".part-source-title").textContent, "Local PDF");
});

test("safeHttpUrl rejects non-http schemes", async () => {
  const { parts } = await boot();
  assert.equal(parts.safeHttpUrl("javascript:alert(1)"), null);
  assert.equal(parts.safeHttpUrl("file:///etc/passwd"), null);
  assert.equal(parts.safeHttpUrl("https://ok.com/x"), "https://ok.com/x");
});

test("artifact renders label/version and fires open hook", async () => {
  const { parts, container } = await boot();
  let opened = null;
  parts.renderParts(
    container,
    [{ kind: "artifact", payload: { id: "art-9", label: "NVDA model", version: 3, kind: "workbook" } }],
    { onOpenArtifact: (a) => (opened = a.id) },
  );
  const node = container.querySelector(".part-artifact");
  assert.equal(node.dataset.artifactId, "art-9");
  assert.equal(node.querySelector(".part-artifact-version").textContent, "v3");
  node.querySelector(".part-artifact-open").click();
  assert.equal(opened, "art-9");
});

test("approval shows Go ahead / Not this time; overwrite adds new version", async () => {
  const { parts, container } = await boot();
  const responses = [];
  parts.renderParts(
    container,
    [{ kind: "approval", payload: { tool_call_id: "w1", name: "export_excel", risk: "export", query: "out.xlsx" } }],
    { onApprove: (id, r) => responses.push([id, r]) },
  );
  const btns = container.querySelectorAll(".part-approval-btn");
  assert.equal(btns.length, 3); // approve_once, deny, create_new_version (export)
  const labels = [...btns].map((b) => b.dataset.response);
  assert.deepEqual(labels, ["approve_once", "deny", "create_new_version"]);
  assert.equal(btns[0].textContent, "Go ahead");
  assert.equal(btns[1].textContent, "Not this time");
  assert.equal(btns[2].textContent, "Save as a new version");
  btns[0].click();
  assert.deepEqual(responses, [["w1", "approve_once"]]);
});

test("read-only-ish approval has only two buttons", async () => {
  const { parts, container } = await boot();
  parts.renderParts(container, [
    { kind: "approval", payload: { tool_call_id: "t1", name: "delete", risk: "local_delete" } },
  ]);
  assert.equal(container.querySelectorAll(".part-approval-btn").length, 2);
});

test("error notice fires retry hook; warning has no retry", async () => {
  const { parts, container } = await boot();
  let retried = false;
  parts.renderParts(
    container,
    [{ kind: "error", payload: { detail: "Timeout" } }],
    { onRetry: () => (retried = true) },
  );
  const err = container.querySelector(".part-error");
  assert.equal(err.getAttribute("role"), "alert");
  assert.equal(err.querySelector(".part-notice-text").textContent, "Timeout");
  err.querySelector(".part-retry").click();
  assert.equal(retried, true);

  const c2 = document.createElement("div");
  parts.renderParts(c2, [{ kind: "warning", payload: { detail: "Heads up" } }]);
  assert.equal(c2.querySelector(".part-retry"), null);
  assert.equal(c2.querySelector(".part-warning").getAttribute("role"), "status");
});

test("result and activity delegate to injected hooks", async () => {
  const { parts, container } = await boot();
  const seen = [];
  parts.renderParts(
    container,
    [
      { kind: "result", payload: { card: { type: "quote", ticker: "AAPL" } } },
      { kind: "activity", payload: { tool_call_id: "a1" } },
    ],
    {
      renderResult: (card) => {
        seen.push(["result", card.ticker]);
        const d = document.createElement("div");
        d.className = "stub-result";
        return d;
      },
      renderActivity: (part) => {
        seen.push(["activity", part.payload.tool_call_id]);
        const d = document.createElement("div");
        d.className = "stub-activity";
        return d;
      },
    },
  );
  assert.deepEqual(seen, [["result", "AAPL"], ["activity", "a1"]]);
  assert.ok(container.querySelector(".stub-result"));
  assert.ok(container.querySelector(".stub-activity"));
});

test("memory_notice uses hook, else fallback", async () => {
  const { parts, container } = await boot();
  parts.renderParts(container, [{ kind: "memory_notice", payload: { count: 2 } }]);
  assert.equal(container.querySelector(".part-memory-text").textContent, "Memory updated · 2");

  const c2 = document.createElement("div");
  let got = null;
  parts.renderParts(c2, [{ kind: "memory_notice", payload: { count: 5 } }], {
    renderMemory: (part) => {
      got = part.payload.count;
      const d = document.createElement("div");
      d.className = "stub-mem";
      return d;
    },
  });
  assert.equal(got, 5);
  assert.ok(c2.querySelector(".stub-mem"));
});

test("unknown kind skipped, surrounding order preserved", async () => {
  const { parts, container } = await boot();
  const n = parts.renderParts(container, [
    { kind: "text", payload: { text: "a" } },
    { kind: "mystery", payload: {} },
    { kind: "text", payload: { text: "b" } },
  ]);
  assert.equal(n, 2);
  assert.equal(container.children.length, 2);
  assert.equal(container.children[0].textContent, "a");
  assert.equal(container.children[1].textContent, "b");
});

test("re-render replaces prior content (idempotent snapshot)", async () => {
  const { parts, container } = await boot();
  parts.renderParts(container, [{ kind: "text", payload: { text: "first" } }]);
  parts.renderParts(container, [{ kind: "text", payload: { text: "second" } }]);
  assert.equal(container.children.length, 1);
  assert.equal(container.children[0].textContent, "second");
});
