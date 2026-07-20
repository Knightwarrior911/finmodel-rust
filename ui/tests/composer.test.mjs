// Composer input surface: model picker type-ahead, attachment chips,
// paste-to-attach, drop claims, and the pure helpers behind them.
import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

const CATALOG = [
  { id: "openai/gpt-4.1-mini", name: "GPT-4.1 Mini", context_length: 1047576, pricing: { prompt: "0.0000004" }, native_tools: true },
  { id: "anthropic/claude-sonnet-4", name: "Claude Sonnet 4", context_length: 200000, pricing: { prompt: "0.000003" }, native_tools: true },
  { id: "anthropic/claude-opus-4", name: "Claude Opus 4", context_length: 200000, pricing: { prompt: "0.000015" }, native_tools: true },
  { id: "deepseek/deepseek-chat", name: "DeepSeek V3", context_length: 64000, pricing: { prompt: "0.0000002" }, native_tools: false },
];

async function bootComposer(ctx) {
  const composer = await importModule("composer.mjs");
  composer.initComposer({ getConversationId: () => "c-test", currentModel: "openai/gpt-4.1-mini" });
  return composer;
}

// ── pure helpers ────────────────────────────────────────────────────

test("filterModels matches every term across id and name, case-insensitive", async () => {
  setupDom();
  const { filterModels } = await importModule("composer.mjs");
  assert.equal(filterModels(CATALOG, "").length, 4);
  assert.equal(filterModels(CATALOG, "claude").length, 2);
  assert.equal(filterModels(CATALOG, "claude son")[0].id, "anthropic/claude-sonnet-4");
  assert.equal(filterModels(CATALOG, "MINI")[0].id, "openai/gpt-4.1-mini");
  assert.equal(filterModels(CATALOG, "zzz").length, 0);
});

test("paste classification: url vs long text vs ordinary", async () => {
  setupDom();
  const { classifyPaste } = await importModule("composer.mjs");
  assert.equal(classifyPaste("https://sec.gov/filing.htm"), "url");
  assert.equal(classifyPaste("check https://a.b and more words"), "text");
  assert.equal(classifyPaste("x".repeat(6001)), "attach");
  assert.equal(classifyPaste("regular question"), "text");
});

test("size, price, and context labels read like a terminal, not a debugger", async () => {
  setupDom();
  const { formatSize, modelPriceLabel, contextLabel } = await importModule("composer.mjs");
  assert.equal(formatSize(843), "843 B");
  assert.equal(formatSize(12_700), "12.4 KB");
  assert.equal(formatSize(3_250_000), "3.1 MB");
  assert.equal(modelPriceLabel(CATALOG[1]), "$3.00/M");
  assert.equal(modelPriceLabel({}), "");
  assert.equal(contextLabel(CATALOG[0]), "1048k ctx");
  assert.equal(contextLabel({}), "");
});

// ── model picker (DOM) ──────────────────────────────────────────────

test("pill opens the live catalog; typing filters; Enter selects and persists", async () => {
  const ctx = setupDom();
  let listed = 0;
  let saved = null;
  ctx.invokeHandlers.list_models = async () => {
    listed += 1;
    return JSON.stringify(CATALOG);
  };
  ctx.invokeHandlers.set_model = async (args) => {
    saved = args.model;
    return JSON.stringify({ model: args.model });
  };
  await bootComposer(ctx);

  document.getElementById("modelPill").click();
  await tick();
  await tick();
  assert.equal(listed, 1, "opening fetches the live catalog");
  const list = document.getElementById("modelPickerList");
  assert.equal(list.querySelectorAll(".mp-row").length, 4);

  const input = document.getElementById("modelPickerInput");
  input.value = "claude opus";
  input.dispatchEvent(new window.Event("input", { bubbles: true }));
  await tick();
  const rows = list.querySelectorAll(".mp-row");
  assert.equal(rows.length, 1);
  assert.match(rows[0].textContent, /Opus/);

  input.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  await tick();
  assert.equal(saved, "anthropic/claude-opus-4", "Enter persists via set_model");
  assert.equal(document.getElementById("modelPillText").textContent, "anthropic/claude-opus-4");
  assert.equal(document.getElementById("modelPicker").hidden, true, "picker closes on select");
});

test("arrow keys move the active row; Escape closes without saving", async () => {
  const ctx = setupDom();
  let saved = null;
  ctx.invokeHandlers.list_models = async () => JSON.stringify(CATALOG);
  ctx.invokeHandlers.set_model = async (a) => ((saved = a.model), "{}");
  await bootComposer(ctx);
  document.getElementById("modelPill").click();
  await tick();
  await tick();
  const input = document.getElementById("modelPickerInput");
  input.dispatchEvent(new window.KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true }));
  await tick();
  const active = document.querySelector(".mp-row.active");
  assert.match(active.textContent, /Sonnet/, "second row active after ArrowDown");
  input.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await tick();
  assert.equal(document.getElementById("modelPicker").hidden, true);
  assert.equal(saved, null, "Escape never saves");
});

test("current model is marked in the list", async () => {
  const ctx = setupDom();
  ctx.invokeHandlers.list_models = async () => JSON.stringify(CATALOG);
  await bootComposer(ctx);
  document.getElementById("modelPill").click();
  await tick();
  await tick();
  const current = document.querySelector(".mp-row.current");
  assert.ok(current, "current model row marked");
  assert.match(current.textContent, /Mini/);
});

// ── attachments: picker button, chips, paste, drop ─────────────────

function fakePng(name = "shot.png") {
  return new File([new Uint8Array([137, 80, 78, 71, 13, 10, 26, 10])], name, {
    type: "image/png",
  });
}

test("staging a file renders a chip with name, size, and remove", async () => {
  const ctx = setupDom();
  let stagedName = null;
  ctx.invokeHandlers.stage_attachment = async (args) => {
    stagedName = args.name;
    assert.equal(args.owner, "c-test", "owner is the open conversation");
    assert.ok(args.bytesB64.length > 0, "bytes travel as base64");
    return JSON.stringify({ artifact_id: "art-1", label: args.name, class: "image", size: 8 });
  };
  const composer = await bootComposer(ctx);
  await composer.stageFile(fakePng());
  await tick();
  assert.equal(stagedName, "shot.png");
  const chip = document.querySelector(".attach-chip");
  assert.ok(chip, "chip rendered");
  assert.match(chip.textContent, /shot\.png/);
  assert.match(chip.textContent, /8 B/);
  assert.equal(composer.attachmentPayload().length, 1);
  assert.equal(composer.attachmentPayload()[0].scope, "c-test");

  chip.querySelector(".attach-remove").click();
  await tick();
  assert.equal(document.querySelectorAll(".attach-chip").length, 0, "× removes the chip");
  assert.equal(composer.attachmentPayload().length, 0);
});

test("Ctrl+V with an image on the clipboard becomes an image attachment", async () => {
  const ctx = setupDom();
  const staged = [];
  ctx.invokeHandlers.stage_attachment = async (args) => {
    staged.push(args.name);
    return JSON.stringify({ artifact_id: `art-${staged.length}`, label: args.name, class: "image", size: 8 });
  };
  await bootComposer(ctx);
  const ta = document.getElementById("chatInput");
  const ev = new window.Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(ev, "clipboardData", {
    value: {
      items: [{ kind: "file", type: "image/png", getAsFile: () => fakePng("clip.png") }],
      getData: () => "",
    },
  });
  ta.dispatchEvent(ev);
  await tick();
  await tick();
  assert.equal(staged.length, 1);
  assert.match(staged[0], /^screenshot-\d{6}\.png$/, "pasted image gets a timestamped name");
  assert.ok(document.querySelector(".attach-chip"), "chip appears");
  assert.equal(ev.defaultPrevented, true, "image paste never dumps into the textarea");
});

test("a very long text paste converts to a text attachment; a URL just hints", async () => {
  const ctx = setupDom();
  const staged = [];
  ctx.invokeHandlers.stage_attachment = async (args) => {
    staged.push(args.name);
    return JSON.stringify({ artifact_id: "art-t", label: args.name, class: "text", size: 9000 });
  };
  await bootComposer(ctx);
  const ta = document.getElementById("chatInput");

  const long = new window.Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(long, "clipboardData", {
    value: { items: [], getData: () => "y".repeat(9000) },
  });
  ta.dispatchEvent(long);
  await tick();
  await tick();
  assert.deepEqual(staged, ["pasted-text.txt"]);
  assert.equal(long.defaultPrevented, true);

  const url = new window.Event("paste", { bubbles: true, cancelable: true });
  Object.defineProperty(url, "clipboardData", {
    value: { items: [], getData: () => "https://www.sec.gov/filing.htm" },
  });
  ta.dispatchEvent(url);
  await tick();
  assert.equal(url.defaultPrevented, false, "URL paste stays in the box");
  const hint = document.getElementById("composerHint");
  assert.equal(hint.hidden, false);
  assert.match(hint.textContent, /read that page/);
});

test("OS drop grants are claimed into chips (multi-file), never raw paths", async () => {
  const ctx = setupDom();
  const grants = [
    { artifact_id: "art-d1", label: "10k.pdf", class: "pdf" },
    { artifact_id: "art-d2", label: "deck.pptx", class: "deck" },
  ];
  ctx.invokeHandlers.claim_dropped_file = async (args) => {
    assert.equal(args.owner, "c-test");
    const g = grants.shift();
    if (!g) throw new Error("no pending drop");
    return JSON.stringify(g);
  };
  const composer = await bootComposer(ctx);
  ctx.emit("file_drop_ready", { count: 2 });
  await tick();
  await tick();
  const chips = document.querySelectorAll(".attach-chip");
  assert.equal(chips.length, 2, "both dropped files become chips");
  assert.match(chips[0].textContent, /10k\.pdf/);
  assert.match(chips[1].textContent, /deck\.pptx/);
  const payload = composer.attachmentPayload();
  assert.deepEqual(
    payload.map((a) => a.class),
    ["pdf", "deck"],
  );
});

// ── prompt polish ───────────────────────────────────────────────────

test("polish rewrites the draft in place; undo restores the original", async () => {
  const ctx = setupDom();
  let sentDraft = null;
  ctx.invokeHandlers.refine_prompt = async (args) => {
    sentDraft = args.draft;
    return JSON.stringify({ text: "What was Tesla's FY2025 automotive revenue, in USD millions?" });
  };
  await bootComposer(ctx);
  const ta = document.getElementById("chatInput");
  ta.value = "tesla revenue?";
  document.getElementById("refineBtn").click();
  await tick();
  await tick();
  assert.equal(sentDraft, "tesla revenue?");
  assert.match(ta.value, /FY2025 automotive revenue/);
  // The hint offers the way back — clicking restores the raw draft.
  const undo = document.querySelector("#composerHint .hint-action");
  assert.ok(undo, "undo affordance shown");
  undo.click();
  assert.equal(ta.value, "tesla revenue?");
});

test("polish with an empty box never calls the model", async () => {
  const ctx = setupDom();
  let called = 0;
  ctx.invokeHandlers.refine_prompt = async () => {
    called += 1;
    return JSON.stringify({ text: "x" });
  };
  await bootComposer(ctx);
  document.getElementById("refineBtn").click();
  await tick();
  assert.equal(called, 0, "no billable call for an empty draft");
  assert.match(document.getElementById("composerHint").textContent, /Type a question/);
});

test("vision-capable models get a 'sees images' badge in the picker", async () => {
  const ctx = setupDom();
  const cat = [
    { ...CATALOG[0], vision: true },
    { ...CATALOG[1], vision: false },
  ];
  ctx.invokeHandlers.list_models = async () => JSON.stringify(cat);
  await bootComposer(ctx);
  document.getElementById("modelPill").click();
  await tick();
  await tick();
  const rows = document.querySelectorAll(".mp-row");
  assert.match(rows[0].innerHTML, /sees images/);
  assert.ok(!/sees images/.test(rows[1].innerHTML));
});
