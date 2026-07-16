// reader.mjs — right-side slide-in reader panel. Renders an explicit state
// (Phase 4.2): loading, ready(-highlighted), ready-no-match, blocked, thin,
// failed, empty, opener-failure. Never a dead end — every non-ready state keeps
// the source reachable (Open externally / Copy URL / Retry).

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
let currentTitle = null;
// Monotonic reader request token: a newer openReader invalidates older awaits.
let readerReqToken = 0;
// Debounce handle for Find (150 ms).
let findTimer = 0;
// Whether the current body has readable, findable text.
let hasFindableText = false;
// Element focus returns to when the reader closes.
let readerReturnFocus = null;

function openCta(url) {
  return `<button type="button" class="btn-primary reader-cta" data-url="${escapeHtml(url)}">Open in browser ↗</button>`;
}

function retryCta() {
  return `<button type="button" class="btn-ghost" data-reader-retry>Retry</button>`;
}

function roamHint() {
  return `<button type="button" class="btn-ghost reader-roam" data-open-settings>Configure the Roam browser in Settings</button>`;
}

function stateBlock(inner) {
  return `<div class="reader-state">${inner}</div>`;
}

function blockedHTML(url) {
  return stateBlock(`
    <p>This site blocks automated reading.</p>
    <p class="reader-hint">Open it in your browser, or configure the Roam browser in Settings for full in-app reading of dynamic and login-gated pages.</p>
    <div class="reader-cta-row">${openCta(url)}${roamHint()}${retryCta()}</div>`);
}

function thinHTML(url) {
  return stateBlock(`
    <p>This page returned little readable text — it's likely JavaScript-heavy or protected.</p>
    <p class="reader-hint">Open it in your browser, or configure the Roam browser in Settings for full in-app reading.</p>
    <div class="reader-cta-row">${openCta(url)}${roamHint()}${retryCta()}</div>`);
}

function emptyHTML(url) {
  return stateBlock(`
    <p>This page returned no readable text.</p>
    <p class="reader-hint">Open it in your browser to view the original.</p>
    <div class="reader-cta-row">${openCta(url)}${retryCta()}</div>`);
}

function failedHTML(url, msg) {
  return stateBlock(`
    <p>Couldn't read this page: ${escapeHtml(msg)}</p>
    <div class="reader-cta-row">${openCta(url)}${retryCta()}</div>`);
}

function openerFailureHTML(url) {
  return stateBlock(`
    <p class="err">Couldn't open your browser.</p>
    <p class="reader-hint">Copy the link and open it manually.</p>
    <div class="reader-cta-row">
      <button type="button" class="btn-primary" data-reader-copy>Copy URL</button>
      ${retryCta()}
    </div>`);
}

/// Banner appended above the preserved content when Find yields no match.
function noMatchBanner(url) {
  return stateBlock(`
    <p class="reader-nomatch" role="status">No match in this page.</p>
    <div class="reader-cta-row">
      <button type="button" class="btn-ghost" data-reader-copy>Copy URL</button>
      ${openCta(url)}
      ${retryCta()}
    </div>`);
}

export function initReader() {
  $("readerClose").addEventListener("click", closeReader);
  $("readerCopy").addEventListener("click", async () => {
    if (!currentUrl) return;
    await copyToClipboard(currentUrl);
    flashBtn($("readerCopy"), "Copied");
  });
  $("readerOpen").addEventListener("click", () => openWithFallback(currentUrl));
  $("readerUrl").addEventListener("click", (e) => {
    e.preventDefault();
    openWithFallback(currentUrl);
  });
  $("readerFind").addEventListener("input", (e) => readerFind(e.target.value.trim()));
  $("readerBody").addEventListener("click", (e) => {
    if (e.target.closest("[data-open-settings]")) {
      document.dispatchEvent(new CustomEvent("open-settings"));
      return;
    }
    if (e.target.closest("[data-reader-retry]")) {
      e.preventDefault();
      if (currentUrl) openReader(currentUrl, currentTitle);
      return;
    }
    if (e.target.closest("[data-reader-copy]")) {
      e.preventDefault();
      if (currentUrl) {
        copyToClipboard(currentUrl);
      }
      return;
    }
    const el = e.target.closest("[data-url]");
    if (el) {
      e.preventDefault();
      openWithFallback(el.dataset.url);
    }
  });
  document.addEventListener("keydown", (e) => {
    if (e.key === "Escape" && !$("readerPanel").hidden) closeReader();
  });
}

/// Open externally; on opener failure show the opener-failure state.
async function openWithFallback(url) {
  const ok = await openExternal(url);
  if (!ok) {
    $("readerBody").innerHTML = openerFailureHTML(url);
    $("readerBody").dataset.state = "opener-failure";
    $("readerFind").hidden = true;
    hasFindableText = false;
  }
}

function setBody(html, state, findable) {
  const body = $("readerBody");
  body.innerHTML = html;
  body.dataset.state = state;
  hasFindableText = !!findable;
  $("readerFind").hidden = !findable;
}

export async function openReader(url, title) {
  const req = ++readerReqToken;
  if (!url) return;
  // Capture focus origin the first time the reader opens (not on in-reader retry).
  if ($("readerPanel").hidden && document.activeElement instanceof HTMLElement) {
    readerReturnFocus = document.activeElement;
  }
  currentUrl = url;
  currentTitle = title || null;
  const dom = domainOf(url);
  const panel = $("readerPanel");
  panel.hidden = false;
  document.body.classList.add("reader-open");
  // rAF so the slide-in transition runs from the hidden state.
  requestAnimationFrame(() => panel.classList.add("open"));
  $("readerTitle").textContent = title || dom;
  $("readerUrl").textContent = dom;
  $("readerUrl").title = url;
  $("readerFind").value = "";
  // Move focus into the dialog (close button) for keyboard users.
  $("readerClose").focus();
  const body = $("readerBody");
  body.setAttribute("aria-busy", "true");
  setBody(
    `<div class="reader-loading"><span class="spinner"></span> Loading ${escapeHtml(dom)}…</div>`,
    "loading",
    false
  );
  try {
    const res = await call("read_page", { url });
    if (req !== readerReqToken) return; // stale
    body.removeAttribute("aria-busy");
    const status = res.status || "ok";
    const text = (res.text || "").trim();
    if (status === "blocked") {
      setBody(blockedHTML(url), "blocked", false);
    } else if (status === "thin") {
      setBody((text ? renderMarkdown(text) : "") + thinHTML(url), "thin", !!text);
    } else if (!text) {
      setBody(emptyHTML(url), "empty", false);
    } else {
      setBody(renderMarkdown(text), "ready", true);
    }
    if (res.title && !title) $("readerTitle").textContent = res.title;
  } catch (e) {
    if (req !== readerReqToken) return; // stale failure must not clobber current
    body.removeAttribute("aria-busy");
    const msg = e && e.message ? e.message : String(e);
    setBody(failedHTML(url, msg), "failed", false);
  }
}

export function closeReader() {
  readerReqToken++; // invalidate any in-flight request
  clearTimeout(findTimer);
  findTimer = 0;
  const panel = $("readerPanel");
  panel.classList.remove("open");
  panel.hidden = true;
  document.body.classList.remove("reader-open");
  $("readerBody").removeAttribute("aria-busy");
  // Return focus to whatever opened the reader.
  if (readerReturnFocus && readerReturnFocus.focus && document.contains(readerReturnFocus)) {
    readerReturnFocus.focus();
  }
  readerReturnFocus = null;
}

// Debounced Find (150 ms) → highlight, or a no-match state that preserves the page.
function readerFind(term) {
  clearTimeout(findTimer);
  findTimer = setTimeout(() => readerFindNow(term), 150);
}

function readerFindNow(term) {
  if (!hasFindableText) return;
  const body = $("readerBody");
  // Remove any prior no-match banner and highlights.
  body.querySelectorAll(".reader-state").forEach((n) => n.remove());
  body.querySelectorAll("mark.find-hit").forEach((m) => m.replaceWith(document.createTextNode(m.textContent)));
  body.normalize();
  body.dataset.state = "ready";
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
  if (first) {
    body.dataset.state = "ready-highlighted";
    if (typeof first.scrollIntoView === "function") {
      first.scrollIntoView({ behavior: "smooth", block: "center" });
    }
  } else {
    // Ready-no-match: preserve the full source, offer find/Copy URL/Retry/Open.
    body.dataset.state = "ready-no-match";
    body.insertAdjacentHTML("afterbegin", noMatchBanner(currentUrl));
  }
}
