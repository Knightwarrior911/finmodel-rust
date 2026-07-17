// parts.mjs — ordered structured message-part renderer (Phase D).
//
// One assistant turn is a backend-ordered list of typed parts. This module
// renders them in that authoritative order so live and reload produce the same
// snapshot. Kinds: text | attachment | activity | tool | result | sources |
// artifact | approval | warning | error | memory_notice.
//
// `result`, `activity`, and `memory_notice` are delegated to injected hooks so
// the app wires the heavy renderers (cards.mjs::renderCard, activity.render,
// memory.render) without this module importing the Tauri bridge. Everything
// else renders here as pure DOM — testable without a browser.

// ── sanitization ────────────────────────────────────────────────────
// Source-derived strings NEVER become raw HTML; clickable URLs are http(s)
// from the trusted ledger only. Mirrors cards.mjs::safeHttpUrl.

function escapeHtml(s) {
  return String(s ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

export function safeHttpUrl(u) {
  const s = String(u ?? "").trim();
  if (!/^https?:\/\//i.test(s)) return null;
  try {
    const url = new URL(s);
    return url.protocol === "http:" || url.protocol === "https:" ? url.href : null;
  } catch (_) {
    return null;
  }
}

function domainOf(u) {
  try {
    return new URL(u).hostname.replace(/^www\./, "");
  } catch (_) {
    return "";
  }
}

/** Normalize a part into `{ kind, p }` where `p` is the payload object. */
function payloadOf(part) {
  return part && typeof part === "object" ? part.payload || part : {};
}

// ── individual part renderers ───────────────────────────────────────

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
}

function renderText(p) {
  const node = el("div", "part-text");
  // Prose is plain text here; the app may replace with a stream-safe Markdown
  // renderer. textContent keeps untrusted model text inert.
  node.textContent = p.text || "";
  return node;
}

function renderAttachment(p) {
  const node = el("div", "part-attachment");
  node.dataset.attachmentId = p.id || p.artifact_id || "";
  const label = el("span", "part-attachment-label", p.label || p.name || "Attachment");
  node.appendChild(label);
  if (p.error) {
    node.classList.add("part-attachment-error");
    node.appendChild(el("span", "part-attachment-errtext", p.error));
  } else if (p.mime) {
    node.appendChild(el("span", "part-attachment-mime", p.mime));
  }
  return node;
}

function renderSources(sources, hooks) {
  const list = Array.isArray(sources) ? sources : [];
  const wrap = el("div", "part-sources");
  wrap.setAttribute("role", "list");
  const head = el("div", "part-sources-head", `Sources · ${list.length}`);
  wrap.appendChild(head);
  list.forEach((s, i) => {
    const row = el("div", "part-source");
    row.setAttribute("role", "listitem");
    row.appendChild(el("span", "part-source-ref", `${i + 1}`));
    const uri = safeHttpUrl(s.canonical_uri || s.url);
    const title = s.title || s.publisher || domainOf(uri || "") || "source";
    if (uri) {
      const a = el("a", "part-source-link", title);
      a.href = uri;
      a.dataset.sourceId = s.id || "";
      a.addEventListener("click", (e) => {
        if (hooks.onOpenSource) {
          e.preventDefault();
          hooks.onOpenSource(s);
        }
      });
      row.appendChild(a);
      row.appendChild(el("span", "part-source-domain", domainOf(uri)));
    } else {
      // Non-http source (filing/local): show title, never a live link.
      row.appendChild(el("span", "part-source-title", title));
    }
    if (s.published_at) row.appendChild(el("span", "part-source-date", s.published_at));
    wrap.appendChild(row);
  });
  return wrap;
}

function renderArtifact(p, hooks) {
  const node = el("div", "part-artifact");
  node.dataset.artifactId = p.id || "";
  node.appendChild(el("span", "part-artifact-label", p.label || p.id || "Artifact"));
  if (p.version != null) node.appendChild(el("span", "part-artifact-version", `v${p.version}`));
  if (p.kind) node.appendChild(el("span", "part-artifact-kind", p.kind));
  const open = el("button", "btn-ghost part-artifact-open", "Open");
  open.type = "button";
  open.addEventListener("click", () => hooks.onOpenArtifact && hooks.onOpenArtifact(p));
  node.appendChild(open);
  return node;
}

function renderApproval(p, hooks) {
  const node = el("div", "part-approval");
  node.dataset.toolCallId = p.tool_call_id || "";
  node.setAttribute("role", "group");
  const risk = p.risk || "";
  node.appendChild(
    el("div", "part-approval-head", `Approval needed: ${p.name || "action"}${risk ? ` · ${risk}` : ""}`),
  );
  if (p.query || p.target) {
    node.appendChild(el("div", "part-approval-target", p.query || p.target));
  }
  const btns = el("div", "part-approval-btns");
  const mk = (label, response, cls) => {
    const b = el("button", `${cls} part-approval-btn`, label);
    b.type = "button";
    b.dataset.response = response;
    b.addEventListener("click", () => hooks.onApprove && hooks.onApprove(p.tool_call_id, response));
    return b;
  };
  btns.appendChild(mk("Approve once", "approve_once", "btn-primary"));
  btns.appendChild(mk("Deny", "deny", "btn-ghost"));
  // Overwrite/export additionally offer a new immutable version.
  if (risk === "local_overwrite" || risk === "export") {
    btns.appendChild(mk("Create new version", "create_new_version", "btn-ghost"));
  }
  node.appendChild(btns);
  return node;
}

function renderNotice(kind, p, hooks) {
  const node = el("div", `part-notice part-${kind}`);
  node.setAttribute("role", kind === "error" ? "alert" : "status");
  node.appendChild(el("span", "part-notice-text", p.detail || p.message || (kind === "error" ? "An error occurred" : "Warning")));
  if (kind === "error" && hooks.onRetry) {
    const retry = el("button", "btn-ghost part-retry", "Retry");
    retry.type = "button";
    retry.addEventListener("click", () => hooks.onRetry(p));
    node.appendChild(retry);
  }
  return node;
}

function renderMemoryFallback(p) {
  const node = el("div", "part-memory");
  const n = p.count != null ? p.count : (p.memoryIds || []).length;
  node.appendChild(el("span", "part-memory-text", `Memory updated · ${n}`));
  return node;
}

// ── dispatch ─────────────────────────────────────────────────────────

/** Render one part to a DOM node, or `null` for unknown/empty kinds. */
export function renderPart(part, hooks = {}) {
  const p = payloadOf(part);
  switch (part.kind) {
    case "text":
      return renderText(p);
    case "attachment":
      return renderAttachment(p);
    case "activity":
      return hooks.renderActivity ? hooks.renderActivity(part) : null;
    case "tool":
    case "result":
      return hooks.renderResult ? hooks.renderResult(p.card || p) : null;
    case "sources":
      return renderSources(p.sources || [], hooks);
    case "artifact":
      return renderArtifact(p, hooks);
    case "approval":
      return renderApproval(p, hooks);
    case "warning":
      return renderNotice("warning", p, hooks);
    case "error":
      return renderNotice("error", p, hooks);
    case "memory_notice":
      return hooks.renderMemory ? hooks.renderMemory(part) : renderMemoryFallback(p);
    default:
      return null; // unknown kinds are skipped; surrounding order is preserved
  }
}

/**
 * Render an ordered list of parts into `container`, preserving backend order.
 * Returns the number of nodes appended.
 *
 * @param {HTMLElement} container
 * @param {Array<{kind: string, payload?: object}>} parts
 * @param {object} [hooks] renderResult, renderActivity, renderMemory,
 *   onOpenSource, onOpenArtifact, onApprove, onRetry
 */
export function renderParts(container, parts, hooks = {}) {
  container.innerHTML = "";
  let n = 0;
  for (const part of Array.isArray(parts) ? parts : []) {
    const node = renderPart(part, hooks);
    if (node) {
      container.appendChild(node);
      n += 1;
    }
  }
  return n;
}

export const _internal = { escapeHtml, domainOf };
