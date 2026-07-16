// harness.mjs — jsdom bootstrap + mocked Tauri bridge for UI regression tests
// (Phase 4.7). Loads the real index.html DOM, installs a controllable
// window.__TAURI__ (invoke + event listen/emit), and exposes helpers to drive
// the same modules the app ships — no second renderer, no real browser.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { JSDOM } from "jsdom";

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = join(here, "..");

/// Extract the <body> inner HTML from index.html (skip the module script).
function bodyHTML() {
  const html = readFileSync(join(uiRoot, "index.html"), "utf8");
  const m = html.match(/<body[^>]*>([\s\S]*?)<\/body>/i);
  let inner = m ? m[1] : "";
  // Drop the module <script> tag — tests import modules directly.
  inner = inner.replace(/<script[\s\S]*?<\/script>/gi, "");
  return inner;
}

/// A controllable Tauri mock. `invokeHandlers[name] = (payload) => result|Promise`.
/// `emit(event, payload)` fires listeners synchronously.
export function makeTauri() {
  const listeners = new Map();
  const invokeHandlers = {};
  const invokeLog = [];
  const tauri = {
    core: {
      invoke: async (name, payload) => {
        invokeLog.push({ name, payload });
        const h = invokeHandlers[name];
        if (!h) throw new Error(`no mock for command: ${name}`);
        const out = await h(payload);
        // App `call()` JSON-parses strings; return objects as JSON strings to
        // mirror the real bridge contract.
        return typeof out === "string" ? out : JSON.stringify(out);
      },
    },
    event: {
      listen: (event, handler) => {
        const arr = listeners.get(event) || [];
        arr.push(handler);
        listeners.set(event, arr);
        return () => {
          const cur = listeners.get(event) || [];
          listeners.set(
            event,
            cur.filter((h) => h !== handler)
          );
        };
      },
    },
  };
  const emit = (event, payload) => {
    for (const h of listeners.get(event) || []) h({ event, payload });
  };
  return { tauri, invokeHandlers, invokeLog, emit };
}

/// Build a fresh jsdom window wired with the app DOM + a Tauri mock.
// Timers scheduled during a test; cleared before the next setup so a stray
// debounce/rAF from a prior test can't mutate the current DOM (shared globals).
let liveTimers = new Set();

export function setupDom({ theme = "light" } = {}) {
  // Kill any timers left pending by the previous test.
  for (const id of liveTimers) clearTimeout(id);
  liveTimers = new Set();
  const dom = new JSDOM(`<!DOCTYPE html><html><body>${bodyHTML()}</body></html>`, {
    pretendToBeVisual: true,
    url: "https://tauri.localhost/",
  });
  const { window } = dom;
  const { tauri, invokeHandlers, invokeLog, emit } = makeTauri();
  window.__TAURI__ = tauri;
  window.document.documentElement.dataset.theme = theme;
  // Track timers so a stray debounce/rAF cannot mutate a later test's DOM.
  // Modules call the module-global `setTimeout`; wrap globalThis's.
  const rawSet = globalThis.__realSetTimeout || globalThis.setTimeout.bind(globalThis);
  globalThis.__realSetTimeout = rawSet;
  const tracked = (fn, ms, ...args) => {
    const id = rawSet(fn, ms, ...args);
    liveTimers.add(id);
    return id;
  };
  globalThis.setTimeout = tracked;
  window.setTimeout = tracked;
  if (!window.requestAnimationFrame) {
    window.requestAnimationFrame = (cb) => tracked(() => cb(Date.now()), 0);
    window.cancelAnimationFrame = (id) => clearTimeout(id);
  }
  // Expose as globals so the ES modules (which reference document/window) bind.
  const setGlobal = (k, v) => {
    try {
      globalThis[k] = v;
    } catch (_) {
      Object.defineProperty(globalThis, k, { value: v, configurable: true, writable: true });
    }
  };
  setGlobal("window", window);
  setGlobal("document", window.document);
  setGlobal("navigator", window.navigator);
  setGlobal("requestAnimationFrame", window.requestAnimationFrame.bind(window));
  setGlobal("cancelAnimationFrame", window.cancelAnimationFrame.bind(window));
  setGlobal("CustomEvent", window.CustomEvent);
  setGlobal("Node", window.Node);
  setGlobal("NodeFilter", window.NodeFilter);
  setGlobal("getComputedStyle", window.getComputedStyle.bind(window));
  setGlobal("HTMLElement", window.HTMLElement);
  setGlobal("localStorage", window.localStorage);
  if (window.crypto) setGlobal("crypto", window.crypto);
  return { dom, window, tauri, invokeHandlers, invokeLog, emit };
}

/// Import a UI module fresh (cache-busted) so each test gets clean state.
export async function importModule(rel) {
  const url = new URL(`../js/${rel}?t=${Math.random()}`, import.meta.url);
  return import(url.href);
}

/// Flush pending microtasks + one rAF tick.
export function tick() {
  return new Promise((resolve) => setTimeout(resolve, 0));
}
