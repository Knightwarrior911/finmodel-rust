import { test } from "node:test";
import assert from "node:assert/strict";
import {
  createTray,
  reduce,
  activeTasks,
  backgroundTasks,
  visibleTasks,
  render,
} from "../js/tasks.mjs";

function env(kind, over = {}) {
  return {
    conversation_id: over.conversation_id || "c1",
    run_id: over.run_id || "r1",
    event: { kind, payload: over.payload || {} },
  };
}

test("RunStarted adds a running task", () => {
  const t = reduce(createTray(), env("RunStarted", { payload: { title: "NVDA earnings" } }));
  assert.equal(activeTasks(t).length, 1);
  assert.equal(activeTasks(t)[0].status, "running");
  assert.equal(activeTasks(t)[0].title, "NVDA earnings");
});

test("terminal events remove from activeTasks", () => {
  let t = reduce(createTray(), env("RunStarted"));
  t = reduce(t, env("RunCompleted"));
  assert.equal(activeTasks(t).length, 0);
  assert.equal(t.byRun.get("r1").status, "completed");
});

test("backgroundTasks excludes focused conversation", () => {
  let t = createTray();
  t = reduce(t, env("RunStarted", { conversation_id: "c1", run_id: "r1" }));
  t = reduce(t, env("RunStarted", { conversation_id: "c2", run_id: "r2" }));
  t = reduce(t, { type: "FocusConversation", conversationId: "c1" });
  const bg = backgroundTasks(t);
  assert.equal(bg.length, 1);
  assert.equal(bg[0].runId, "r2");
});

test("visibleTasks caps at 3", () => {
  let t = createTray();
  for (let i = 0; i < 5; i++) {
    t = reduce(t, env("RunStarted", { conversation_id: `c${i}`, run_id: `r${i}` }));
  }
  assert.equal(activeTasks(t).length, 5);
  assert.equal(visibleTasks(t).length, 3);
});

test("ApprovalRequested marks awaiting_approval", () => {
  let t = reduce(createTray(), env("RunStarted"));
  t = reduce(t, env("ApprovalRequested"));
  assert.equal(t.byRun.get("r1").status, "awaiting_approval");
});

test("DismissTask removes a row", () => {
  let t = reduce(createTray(), env("RunStarted"));
  t = reduce(t, { type: "DismissTask", runId: "r1" });
  assert.equal(t.byRun.size, 0);
});

test("SubagentUpdate running shows a task with its label", () => {
  const t = reduce(createTray(), {
    type: "SubagentUpdate",
    runId: "sub:r1:1",
    title: "get_financials · AAPL",
    status: "running",
    conversationId: "c1",
  });
  assert.equal(activeTasks(t).length, 1);
  assert.equal(activeTasks(t)[0].title, "get_financials · AAPL");
  assert.equal(activeTasks(t)[0].status, "running");
});

test("SubagentUpdate done is terminal (drops from activeTasks)", () => {
  let t = reduce(createTray(), {
    type: "SubagentUpdate",
    runId: "sub:r1:1",
    title: "get_financials · AAPL",
    status: "running",
  });
  t = reduce(t, { type: "SubagentUpdate", runId: "sub:r1:1", title: "get_financials", status: "done" });
  assert.equal(activeTasks(t).length, 0);
  assert.equal(t.byRun.get("sub:r1:1").status, "completed");
});

test("render writes rows and wires cancel", () => {
  // Minimal DOM stub.
  globalThis.document = {
    createElement(tag) {
      const el = {
        tagName: tag.toUpperCase(),
        className: "",
        textContent: "",
        title: "",
        dataset: {},
        children: [],
        style: {},
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
  let t = reduce(createTray(), env("RunStarted", { payload: { title: "A" } }));
  const root = { innerHTML: "x", hidden: false, appendChild(c) { this.child = c; } };
  let cancelled = null;
  const n = render(root, t, {
    onCancel: (c, r) => {
      cancelled = [c, r];
    },
  });
  assert.equal(n, 1);
  assert.equal(root.hidden, false);
  const cancelBtn = root.child.children.find((c) => c.className.includes("task-cancel"));
  assert.ok(cancelBtn);
  cancelBtn.onclick({ stopPropagation() {} });
  assert.deepEqual(cancelled, ["c1", "r1"]);
});

test("unknown event returns same reference", () => {
  const t = createTray();
  assert.equal(reduce(t, env("AssistantTextDelta")), t);
});
