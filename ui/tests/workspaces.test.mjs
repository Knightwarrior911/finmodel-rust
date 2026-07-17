import { test } from "node:test";
import assert from "node:assert/strict";
import {
  createWorkspaceState,
  reduce,
  activeWorkspace,
  bannerText,
  render,
} from "../js/workspaces.mjs";

test("default Personal workspace is standard", () => {
  const s = createWorkspaceState();
  assert.equal(activeWorkspace(s).id, "personal");
  assert.equal(activeWorkspace(s).confidentiality, "standard");
  assert.equal(bannerText(s), "");
});

test("SetWorkspaces replaces list and keeps active when present", () => {
  let s = createWorkspaceState();
  s = reduce(s, {
    type: "SetWorkspaces",
    workspaces: [
      { id: "deal-1", name: "Project Falcon", confidentiality: "confidential" },
      { id: "personal", name: "Personal", confidentiality: "standard" },
    ],
  });
  assert.equal(s.list.length, 2);
  assert.equal(s.activeId, "personal");
});

test("SelectWorkspace clears Temporary", () => {
  let s = createWorkspaceState();
  s = reduce(s, { type: "ToggleTemporary", value: true });
  assert.equal(s.temporary, true);
  s = reduce(s, {
    type: "SetWorkspaces",
    workspaces: [
      { id: "a", name: "A", confidentiality: "confidential" },
      { id: "b", name: "B", confidentiality: "standard" },
    ],
  });
  s = reduce(s, { type: "SelectWorkspace", id: "a" });
  assert.equal(s.activeId, "a");
  assert.equal(s.temporary, false);
});

test("confidential and restricted banners", () => {
  let s = reduce(createWorkspaceState(), {
    type: "SetWorkspaces",
    workspaces: [{ id: "d", name: "Deal", confidentiality: "confidential" }],
  });
  s = reduce(s, { type: "SelectWorkspace", id: "d" });
  assert.match(bannerText(s), /Confidential/);
  s = reduce(s, { type: "SetConfidentiality", confidentiality: "restricted" });
  assert.match(bannerText(s), /Restricted/);
});

test("Temporary banner overrides workspace tier", () => {
  let s = reduce(createWorkspaceState(), {
    type: "SetWorkspaces",
    workspaces: [{ id: "d", name: "Deal", confidentiality: "confidential" }],
  });
  s = reduce(s, { type: "SelectWorkspace", id: "d" });
  s = reduce(s, { type: "ToggleTemporary", value: true });
  assert.match(bannerText(s), /Temporary Chat/);
});

test("invalid confidentiality is ignored", () => {
  const s0 = createWorkspaceState();
  const s1 = reduce(s0, { type: "SetConfidentiality", confidentiality: "public" });
  assert.equal(s1, s0);
});

test("render updates select banner and temp button", () => {
  globalThis.document = {
    createElement(tag) {
      return {
        tagName: tag.toUpperCase(),
        value: "",
        textContent: "",
        selected: false,
        appendChild() {},
      };
    },
  };
  let s = reduce(createWorkspaceState(), {
    type: "SetWorkspaces",
    workspaces: [{ id: "d", name: "Deal", confidentiality: "confidential" }],
  });
  s = reduce(s, { type: "SelectWorkspace", id: "d" });
  const select = { innerHTML: "", appendChild() { this.n = (this.n || 0) + 1; }, onchange: null };
  const banner = { textContent: "", hidden: true, dataset: {} };
  const tempBtn = {
    textContent: "",
    classList: { toggle() {} },
    setAttribute() {},
    onclick: null,
  };
  let selected = null;
  let toggled = false;
  render({ select, banner, tempBtn }, s, {
    onSelect: (id) => {
      selected = id;
    },
    onToggleTemporary: () => {
      toggled = true;
    },
  });
  assert.equal(select.n, 1);
  assert.match(banner.textContent, /Confidential/);
  assert.equal(banner.hidden, false);
  tempBtn.onclick();
  assert.equal(toggled, true);
  assert.equal(typeof select.onchange, "function");
  select.value = "d";
  select.onchange();
  assert.equal(selected, "d");
});
