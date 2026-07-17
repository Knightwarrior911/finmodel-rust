import { test } from "node:test";
import assert from "node:assert/strict";
import {
  createMemoryUi,
  reduce,
  noticeText,
  undoOpen,
  render,
  UNDO_WINDOW_MS,
} from "../js/memory.mjs";

test("MemoryUpdated creates undoable notice", () => {
  const now = 1_000_000;
  const s = reduce(createMemoryUi(), {
    type: "MemoryUpdated",
    count: 2,
    memoryIds: ["m1", "m2"],
    provenance: "user preference",
    now,
  });
  assert.match(noticeText(s), /Memory updated · 2/);
  assert.equal(undoOpen(s, now + 100), true);
  assert.equal(undoOpen(s, now + UNDO_WINDOW_MS + 1), false);
});

test("Temporary Chat suppresses MemoryUpdated", () => {
  let s = reduce(createMemoryUi(), { type: "SetTemporary", value: true });
  s = reduce(s, { type: "MemoryUpdated", count: 1, memoryIds: ["m1"], now: 1 });
  assert.equal(s.notice, null);
  assert.equal(noticeText(s), "");
});

test("Undo within window marks undone and clears text", () => {
  const now = 50;
  let s = reduce(createMemoryUi(), {
    type: "MemoryUpdated",
    count: 1,
    memoryIds: ["m1"],
    now,
  });
  s = reduce(s, { type: "UndoNotice", now: now + 500 });
  assert.equal(s.notice.undone, true);
  assert.equal(noticeText(s), "");
});

test("Undo after window is a no-op", () => {
  const now = 50;
  let s = reduce(createMemoryUi(), {
    type: "MemoryUpdated",
    count: 1,
    memoryIds: ["m1"],
    now,
  });
  const before = s;
  s = reduce(s, { type: "UndoNotice", now: now + UNDO_WINDOW_MS + 10 });
  assert.equal(s, before);
  assert.equal(s.notice.undone, false);
});

test("Dismiss clears notice", () => {
  let s = reduce(createMemoryUi(), {
    type: "MemoryUpdated",
    count: 3,
    now: 1,
  });
  s = reduce(s, { type: "DismissNotice" });
  assert.equal(s.notice, null);
});

test("history keeps recent notices capped", () => {
  let s = createMemoryUi();
  for (let i = 0; i < 25; i++) {
    s = reduce(s, {
      type: "MemoryUpdated",
      count: 1,
      memoryIds: [`m${i}`],
      id: `n${i}`,
      now: i,
    });
  }
  assert.equal(s.history.length, 20);
  assert.equal(s.history[0].id, "n24");
});

test("render shows Undo only while window open", () => {
  globalThis.document = {
    createElement(tag) {
      const el = {
        tagName: tag.toUpperCase(),
        className: "",
        textContent: "",
        children: [],
        setAttribute() {},
        addEventListener(type, fn) {
          this[`on${type}`] = fn;
        },
        appendChild(c) {
          this.children.push(c);
          return c;
        },
      };
      return el;
    },
  };
  const now = 1000;
  const s = reduce(createMemoryUi(), {
    type: "MemoryUpdated",
    count: 1,
    memoryIds: ["m1"],
    now,
  });
  const root = {
    hidden: true,
    _html: "x",
    className: "",
    children: [],
    setAttribute() {},
    get innerHTML() { return this._html; },
    set innerHTML(v) { this._html = v; this.children = []; },
    appendChild(c) { this.children.push(c); return c; },
  };
  let undone = false;
  render(root, s, {
    now: now + 10,
    onUndo: () => {
      undone = true;
    },
  });
  assert.equal(root.hidden, false);
  const undo = root.children.find((c) => c.className.includes("memory-undo"));
  assert.ok(undo);
  undo.onclick();
  assert.equal(undone, true);

  render(root, s, { now: now + UNDO_WINDOW_MS + 1 });
  assert.equal(
    (root.children || []).some((c) => c.className.includes("memory-undo")),
    false,
  );
});
