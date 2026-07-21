// core.mjs — shared primitives: Tauri bridge, sanitized markdown, theme, format.

export const TAURI = window.__TAURI__;
// Resolve the bridge at call time (not module load) so it is robust to late
// injection and testable with a mocked window.__TAURI__.
function bridge() {
  return (typeof window !== "undefined" && window.__TAURI__) || TAURI || null;
}

// Every command returns a JSON *string* (or throws an AppError object).
export async function call(name, payload = {}) {
  const t = bridge();
  if (!t || !t.core || !t.core.invoke)
    throw new Error("Not running inside the app window.");
  const res = await t.core.invoke(name, payload);
  if (typeof res !== "string") return res;
  try {
    return JSON.parse(res);
  } catch (_) {
    return res; // command returned a plain (non-JSON) string
  }
}

export const $ = (id) => document.getElementById(id);

// Subscribe to a Tauri event; returns an unsubscribe fn (no-op outside Tauri).
export function on(event, handler) {
  const t = bridge();
  if (t && t.event && t.event.listen) return t.event.listen(event, handler);
  return () => {};
}

// Escape untrusted values before any innerHTML interpolation.
export function escapeHtml(s) {
  return String(s == null ? "" : s).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
}

// Remove model pseudo-control tokens (e.g. <|eom|>, <|eot_id|>) some models leak.
export function stripControlTokens(s) {
  return String(s == null ? "" : s)
    .replace(/<\|[^|>]*\|>/g, "")
    .trim();
}

// Sanitized markdown → HTML. Escape EVERYTHING first, then re-inject only a
// whitelist: headings, paragraphs, ordered/unordered lists, fenced code blocks,
// GFM pipe tables, and http(s) links. No raw HTML, no <script>/on* survives.
export function renderMarkdown(md) {
  const esc = escapeHtml(String(md == null ? "" : md));
  const lines = esc.split(/\r?\n/);
  const out = [];
  let listType = null; // "ul" | "ol" | null
  const closeList = () => {
    if (listType) {
      out.push(`</${listType}>`);
      listType = null;
    }
  };
  const inline = (t) =>
    t
      .replace(
        /\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
        (_m, txt, url) =>
          `<a href="#" class="md-link" data-url="${url}">${txt}</a>`,
      )
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/`([^`]+)`/g, "<code>$1</code>");

  const isTableSep = (s) =>
    /^\|?\s*:?-{2,}:?\s*(\|\s*:?-{2,}:?\s*)+\|?$/.test(s.trim());
  const splitRow = (s) =>
    s
      .trim()
      .replace(/^\|/, "")
      .replace(/\|$/, "")
      .split("|")
      .map((c) => c.trim());

  for (let i = 0; i < lines.length; i++) {
    const raw = lines[i];
    const line = raw.trim();

    // Fenced code block.
    const fence = line.match(/^```/);
    if (fence) {
      closeList();
      const body = [];
      i++;
      while (i < lines.length && !/^```/.test(lines[i].trim())) {
        body.push(lines[i]);
        i++;
      }
      out.push(`<pre><code>${body.join("\n")}</code></pre>`);
      continue;
    }

    // GFM table: header line with pipes + a separator on the next line.
    if (
      line.includes("|") &&
      i + 1 < lines.length &&
      isTableSep(lines[i + 1])
    ) {
      closeList();
      const headers = splitRow(line);
      i += 2; // skip header + separator
      const rows = [];
      while (
        i < lines.length &&
        lines[i].includes("|") &&
        lines[i].trim() !== ""
      ) {
        rows.push(splitRow(lines[i]));
        i++;
      }
      i--; // step back; loop will advance
      const thead = `<thead><tr>${headers.map((h) => `<th>${inline(h)}</th>`).join("")}</tr></thead>`;
      const tbody = `<tbody>${rows
        .map(
          (r) => `<tr>${r.map((c) => `<td>${inline(c)}</td>`).join("")}</tr>`,
        )
        .join("")}</tbody>`;
      out.push(`<table class="md-table">${thead}${tbody}</table>`);
      continue;
    }

    const h = line.match(/^(#{1,6})\s+(.*)$/);
    const ul = line.match(/^[-*]\s+(.*)$/);
    const ol = line.match(/^\d+\.\s+(.*)$/);

    if (h) {
      closeList();
      const n = Math.min(h[1].length, 6);
      out.push(`<h${n}>${inline(h[2])}</h${n}>`);
    } else if (ul) {
      if (listType !== "ul") {
        closeList();
        out.push("<ul>");
        listType = "ul";
      }
      out.push(`<li>${inline(ul[1])}</li>`);
    } else if (ol) {
      if (listType !== "ol") {
        closeList();
        out.push("<ol>");
        listType = "ol";
      }
      out.push(`<li>${inline(ol[1])}</li>`);
    } else if (line === "") {
      closeList();
    } else {
      closeList();
      out.push(`<p>${inline(line)}</p>`);
    }
  }
  closeList();
  return out.join("");
}

export function domainOf(url) {
  try {
    return new URL(url).hostname.replace(/^www\./, "");
  } catch (_) {
    return url;
  }
}

/// Deep source link via the Text Fragments standard (#:~:text=...): a
/// Chromium/Safari/recent-Firefox browser opens the page SCROLLED TO the
/// quoted passage and highlights it - click a cited figure, land on the
/// exact sentence it came from. Unsupported browsers simply ignore the
/// fragment and open the page (the plain URL is always the fallback).
///
/// Quote hygiene, because fragments fail silently on any mismatch:
/// - search snippets carry ellipses and normalized whitespace, so we split
///   on ellipses and anchor on the LONGEST clean run;
/// - the run is capped at 10 words (long quotes over-constrain matching);
/// - '-', ',' and '&' are directive syntax and get percent-encoded.
export function deepSourceUrl(url, quote) {
  const base = String(url || "");
  if (!/^https?:\/\//i.test(base)) return "";
  const text = fragmentQuote(quote);
  if (!text) return base;
  const enc = encodeURIComponent(text).replace(/-/g, "%2D");
  return base.includes("#") ? `${base}:~:text=${enc}` : `${base}#:~:text=${enc}`;
}

/// The anchorable part of a quote (see deepSourceUrl). Empty when nothing
/// clean enough survives.
export function fragmentQuote(quote) {
  const raw = String(quote || "").replace(/\s+/g, " ").trim();
  if (!raw) return "";
  // Longest run between ellipses (search snippets stitch fragments).
  const runs = raw
    .split(/\u2026|\.\.\./)
    .map((r) => r.trim())
    .filter(Boolean);
  if (!runs.length) return "";
  let best = runs[0];
  for (const r of runs) if (r.length > best.length) best = r;
  // Strip wrapping quotes/brackets, cap at 10 words.
  const cleaned = best.replace(/^["'\u201c\u2018([{\s]+|["'\u201d\u2019)\]}.,;:\s]+$/g, "");
  const words = cleaned.split(" ").filter(Boolean).slice(0, 10);
  const out = words.join(" ");
  // Too short anchors match everywhere or nowhere - not worth a directive.
  return out.length >= 4 ? out : "";
}

/// Open a URL in the OS browser. Returns a promise resolving to `true` on
/// success, `false` on failure (callers may surface an opener-failure state).
export function openExternal(url) {
  if (!url) return Promise.resolve(false);
  return call("open_url", { url })
    .then(() => true)
    .catch(() => false);
}

/// Open a local path via the OS. Resolves `true` on success, `false` on
/// failure (empty path, unregistered artifact, or opener error) so callers
/// can surface a dead-click instead of swallowing it.
export function openPath(path) {
  if (!path) return Promise.resolve(false);
  return call("open_path", { path })
    .then(() => true)
    .catch(() => false);
}

export function flashBtn(btn, txt) {
  const orig = btn.dataset.orig || btn.textContent;
  btn.dataset.orig = orig;
  btn.textContent = txt;
  setTimeout(() => {
    btn.textContent = btn.dataset.orig;
  }, 1200);
}

export async function copyToClipboard(text) {
  try {
    await navigator.clipboard.writeText(text);
  } catch (_) {
    const ta = document.createElement("textarea");
    ta.value = text;
    document.body.appendChild(ta);
    ta.select();
    try {
      document.execCommand("copy");
    } catch (_) {
      /* ignore */
    }
    ta.remove();
  }
}

// ── Theme ───────────────────────────────────────────────────────────
export function currentTheme() {
  const stored = localStorage.getItem("theme");
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
    ? "dark"
    : "light";
}

export function applyTheme(t) {
  document.documentElement.dataset.theme = t;
}

// "system" removes the stored key; "light"/"dark" persist an explicit choice.
export function setTheme(choice) {
  if (choice === "system") localStorage.removeItem("theme");
  else localStorage.setItem("theme", choice);
  applyTheme(currentTheme());
}

export function themeChoice() {
  const stored = localStorage.getItem("theme");
  return stored === "light" || stored === "dark" ? stored : "system";
}

export function toggleTheme() {
  const next = currentTheme() === "dark" ? "light" : "dark";
  setTheme(next);
  return next;
}

export function initTheme() {
  applyTheme(currentTheme());
  if (window.matchMedia) {
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const follow = () => {
      if (!localStorage.getItem("theme")) applyTheme(currentTheme());
    };
    if (mq.addEventListener) mq.addEventListener("change", follow);
  }
}

// ── Formatting (tabular-nums friendly) ──────────────────────────────
export function fmtNum(v) {
  if (v === null || v === undefined) return "—";
  const n = Number(v);
  if (!isFinite(n)) return "—";
  return n.toLocaleString(undefined, { maximumFractionDigits: 1 });
}
export function fmtPct(v) {
  return v == null ? "—" : (Number(v) * 100).toFixed(1) + "%";
}
export function fmtPrice(v) {
  return v == null ? "—" : Number(v).toFixed(2);
}
export function fmtMoneyM(v) {
  return v == null ? "—" : Math.round(Number(v) / 1e6).toLocaleString() + "M";
}
export function relTime(iso) {
  if (!iso) return "";
  const then = Date.parse(iso);
  if (isNaN(then)) return "";
  const secs = Math.max(0, (Date.now() - then) / 1000);
  if (secs < 60) return "now";
  const mins = secs / 60;
  if (mins < 60) return `${Math.floor(mins)}m`;
  const hrs = mins / 60;
  if (hrs < 24) return `${Math.floor(hrs)}h`;
  const days = hrs / 24;
  if (days < 7) return `${Math.floor(days)}d`;
  return new Date(then).toLocaleDateString();
}

// ── Modal dialog a11y (Phase 4.3) ───────────────────────────────────
// Focus trap + background `inert` + focus return. `dialog` is the element that
// receives role=dialog/aria-modal; `opts.initialFocus` is focused on open;
// `opts.onEscape` runs on Escape. Returns a `deactivate()` that restores focus
// and clears inert. Background = every direct child of <body> except the
// dialog's own top-level ancestor.
const FOCUSABLE =
  'a[href], button:not([disabled]), textarea:not([disabled]), input:not([disabled]), select:not([disabled]), [tabindex]:not([tabindex="-1"])';

export function activateDialog(dialog, opts = {}) {
  const returnTo =
    opts.returnFocus ||
    (document.activeElement instanceof HTMLElement
      ? document.activeElement
      : null);
  // Inert every body child that does not contain the dialog.
  const bodyKids = Array.from(document.body.children);
  const inerted = [];
  for (const el of bodyKids) {
    if (el === dialog || el.contains(dialog)) continue;
    if (!el.hasAttribute("inert")) {
      el.setAttribute("inert", "");
      inerted.push(el);
    }
  }
  const focusables = () =>
    Array.from(dialog.querySelectorAll(FOCUSABLE)).filter(
      (el) => el.offsetParent !== null || el === document.activeElement,
    );
  const onKeydown = (e) => {
    if (e.key === "Escape") {
      e.preventDefault();
      if (opts.onEscape) opts.onEscape();
      return;
    }
    if (e.key !== "Tab") return;
    const f = focusables();
    if (f.length === 0) {
      e.preventDefault();
      return;
    }
    const first = f[0];
    const last = f[f.length - 1];
    const active = document.activeElement;
    if (e.shiftKey && (active === first || !dialog.contains(active))) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && active === last) {
      e.preventDefault();
      first.focus();
    }
  };
  dialog.addEventListener("keydown", onKeydown);
  // Initial focus.
  const init =
    (opts.initialFocus && dialog.querySelector(opts.initialFocus)) ||
    focusables()[0] ||
    dialog;
  if (init && init.focus) init.focus();
  return function deactivate() {
    dialog.removeEventListener("keydown", onKeydown);
    for (const el of inerted) el.removeAttribute("inert");
    if (returnTo && returnTo.focus && document.contains(returnTo))
      returnTo.focus();
  };
}
