// reader.mjs — right-side slide-in reader panel. Fetches read_page and renders
// by status: ok → markdown; blocked/thin → honest prompt (never a dead end).

import {
  $,
  call,
  renderMarkdown,
  escapeHtml,
  domainOf,
  openExternal,
  flashBtn,
  copyToClipboard,
} from "./core.mjs";

let currentUrl = null;

function openCta(url) {
  return `<button type="button" class="btn-primary reader-cta" data-url="${escapeHtml(url)}">Open in browser ↗</button>`;
}

function roamHint() {
  return `<button type="button" class="btn-ghost reader-roam" data-open-settings>Configure the Roam browser in Settings</button>`;
}

function blockedHTML(url) {
  return `<div class="reader-blocked">
    <p>This site blocks automated reading.</p>
    <p class="reader-hint">Open it in your browser, or configure the Roam browser in Settings for full in-app reading of dynamic and login-gated pages.</p>
    <div class="reader-cta-row">${openCta(url)}${roamHint()}</div>
  </div>`;
}

function thinHTML(url) {
  return `<div class="reader-blocked">
    <p>This page returned little readable text — it's likely JavaScript-heavy or protected.</p>
    <p class="reader-hint">Open it in your browser, or configure the Roam browser in Settings for full in-app reading.</p>
    <div class="reader-cta-row">${openCta(url)}${roamHint()}</div>
  </div>`;
}

export function initReader() {
  $("readerClose").addEventListener("click", closeReader);
  $("readerCopy").addEventListener("click", async () => {
    if (!currentUrl) return;
    await copyToClipboard(currentUrl);
    flashBtn($("readerCopy"), "Copied");
  });
  $("readerOpen").addEventListener("click", () => openExternal(currentUrl));
  $("readerUrl").addEventListener("click", (e) => {
    e.preventDefault();
    openExternal(currentUrl);
  });
  $("readerFind").addEventListener("input", (e) => readerFind(e.target.value.trim()));
  $("readerBody").addEventListener("click", (e) => {
    if (e.target.closest("[data-open-settings]")) {
      document.dispatchEvent(new CustomEvent("open-settings"));
      return;
    }
    const el = e.target.closest("[data-url]");
    if (el) {
      e.preventDefault();
      openExternal(el.dataset.url);
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("readerPanel").hidden) closeReader();
  });
}

export async function openReader(url, title) {
  if (!url) return;
  currentUrl = url;
  const dom = domainOf(url);
  const panel = $("readerPanel");
  panel.hidden = false;
  // rAF so the slide-in transition runs from the hidden state.
  requestAnimationFrame(() => panel.classList.add("open"));
  $("readerTitle").textContent = title || dom;
  $("readerUrl").textContent = dom;
  $("readerUrl").title = url;
  $("readerFind").hidden = true;
  $("readerFind").value = "";
  $("readerBody").innerHTML = `<div class="reader-loading"><span class="spinner"></span> Loading ${escapeHtml(dom)}…</div>`;
  try {
    const res = await call("read_page", { url });
    const status = res.status || "ok";
    const text = (res.text || "").trim();
    if (status === "blocked") {
      $("readerBody").innerHTML = blockedHTML(url);
    } else if (status === "thin") {
      $("readerBody").innerHTML = (text ? renderMarkdown(text) : "") + thinHTML(url);
      if (text) $("readerFind").hidden = false;
    } else {
      $("readerBody").innerHTML = renderMarkdown(text);
      $("readerFind").hidden = false;
    }
    if (res.title && !title) $("readerTitle").textContent = res.title;
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    $("readerBody").innerHTML = `<div class="reader-error"><p>Couldn't read this page: ${escapeHtml(
      msg
    )}</p>${openCta(url)}</div>`;
  }
}

export function closeReader() {
  const panel = $("readerPanel");
  panel.classList.remove("open");
  panel.hidden = true;
}

// Highlight + scroll to the first match of `term` in the reader body.
function readerFind(term) {
  const body = $("readerBody");
  body.querySelectorAll("mark.find-hit").forEach((m) => m.replaceWith(document.createTextNode(m.textContent)));
  body.normalize();
  if (!term) return;
  const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
  const low = term.toLowerCase();
  const nodes = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  let first = null;
  for (const n of nodes) {
    const idx = n.nodeValue.toLowerCase().indexOf(low);
    if (idx === -1) continue;
    const range = document.createRange();
    range.setStart(n, idx);
    range.setEnd(n, idx + term.length);
    const mark = document.createElement("mark");
    mark.className = "find-hit";
    try {
      range.surroundContents(mark);
      if (!first) first = mark;
    } catch (_) {
      /* skip cross-node match */
    }
  }
  if (first) first.scrollIntoView({ behavior: "smooth", block: "center" });
}
