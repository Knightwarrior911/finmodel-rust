// chat.mjs — the chat pane: composer, streaming send flow, message rendering.

import { $, call, on, escapeHtml, renderMarkdown, stripControlTokens, copyToClipboard, flashBtn } from "./core.mjs";
import { renderCard } from "./cards.mjs";

let currentId = null;
let streaming = false;
let onChanged = () => {};
let activeTurn = null; // { assistantNode, pending: Map<name, node[]> }

export function getCurrentId() {
  return currentId;
}

function scrollEl() {
  return $("chatScroll");
}

function nearBottom() {
  const el = scrollEl();
  return el.scrollHeight - el.scrollTop - el.clientHeight < 48;
}

function scrollToBottom(force) {
  const el = scrollEl();
  if (force || nearBottom()) el.scrollTop = el.scrollHeight;
}

function showEmpty(show) {
  const e = $("chatEmpty");
  if (e) e.hidden = !show;
}

function clearMessages() {
  const el = scrollEl();
  el.querySelectorAll(".msg, .tool-status").forEach((n) => n.remove());
}

// ── message builders ────────────────────────────────────────────────
function appendUser(text) {
  const div = document.createElement("div");
  div.className = "msg msg-user";
  const bubble = document.createElement("div");
  bubble.className = "bubble";
  bubble.textContent = text;
  div.appendChild(bubble);
  scrollEl().appendChild(div);
  scrollToBottom(true);
  return div;
}

function appendAssistant(text, live) {
  const div = document.createElement("div");
  div.className = "msg msg-assistant";
  const prose = document.createElement("div");
  prose.className = "prose";
  if (live) {
    div.setAttribute("aria-live", "polite");
    div.classList.add("streaming");
    prose.dataset.raw = text || "";
    prose.textContent = text || "";
  } else {
    prose.dataset.raw = text || "";
    prose.innerHTML = renderMarkdown(stripControlTokens(text || ""));
  }
  div.appendChild(prose);
  scrollEl().appendChild(div);
  if (!live) addCopyAction(div, text || "");
  return div;
}

function appendCard(card) {
  const div = document.createElement("div");
  div.className = "msg msg-card";
  div.appendChild(renderCard(card));
  scrollEl().appendChild(div);
  scrollToBottom();
  return div;
}

// Hover Copy action on an assistant message — copies the raw markdown source.
function addCopyAction(node, raw) {
  if (!raw || node.querySelector(".msg-copy")) return;
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "msg-copy icon-btn";
  btn.textContent = "Copy";
  btn.title = "Copy message";
  btn.addEventListener("click", () => {
    copyToClipboard(raw);
    flashBtn(btn, "Copied");
  });
  node.appendChild(btn);
}

function toolStatusNode(name) {
  const div = document.createElement("div");
  div.className = "tool-status";
  div.innerHTML = `<span class="spinner"></span><span class="tool-status-name">${escapeHtml(
    name
  )}…</span>`;
  scrollEl().appendChild(div);
  scrollToBottom();
  return div;
}

// ── event stream wiring (single-flight → route all to the active turn) ──
function handleDelta(payload) {
  if (!activeTurn) return;
  if (!activeTurn.assistantNode) {
    activeTurn.assistantNode = appendAssistant("", true);
  }
  const prose = activeTurn.assistantNode.querySelector(".prose");
  prose.dataset.raw = (prose.dataset.raw || "") + (payload.text || "");
  prose.textContent = prose.dataset.raw;
  scrollToBottom();
}

function handleTool(payload) {
  if (!activeTurn) return;
  const name = payload.name || "tool";
  if (payload.status === "start") {
    const node = toolStatusNode(name);
    const list = activeTurn.pending.get(name) || [];
    list.push(node);
    activeTurn.pending.set(name, list);
    return;
  }
  // done | error → replace the earliest pending status for this name.
  const list = activeTurn.pending.get(name) || [];
  const node = list.shift();
  activeTurn.pending.set(name, list);
  if (payload.status === "error" && !payload.card) {
    const errNode = document.createElement("div");
    errNode.className = "msg msg-card";
    errNode.innerHTML = `<div class="card card-error"><p class="card-note err">${escapeHtml(
      payload.detail || (name + " failed")
    )}</p></div>`;
    if (node) node.replaceWith(errNode);
    else scrollEl().appendChild(errNode);
    scrollToBottom();
    return;
  }
  const cardMsg = document.createElement("div");
  cardMsg.className = "msg msg-card";
  cardMsg.appendChild(renderCard(payload.card));
  if (node) node.replaceWith(cardMsg);
  else scrollEl().appendChild(cardMsg);
  scrollToBottom();
}

// Finalize the live assistant bubble: render its accumulated text as markdown.
function finalizeLive() {
  if (activeTurn && activeTurn.assistantNode) {
    activeTurn.assistantNode.classList.remove("streaming");
    const prose = activeTurn.assistantNode.querySelector(".prose");
    const clean = stripControlTokens(prose.dataset.raw || "");
    if (clean === "") {
      activeTurn.assistantNode.remove();
    } else {
      prose.innerHTML = renderMarkdown(clean);
      addCopyAction(activeTurn.assistantNode, clean);
    }
  }
  activeTurn = null;
}

// ── public flows ────────────────────────────────────────────────────
export async function loadConversation(id) {
  if (streaming) return;
  try {
    const conv = await call("load_conversation", { id });
    currentId = conv.id;
    clearMessages();
    showEmpty(false);
    for (const m of conv.messages || []) {
      if (m.role === "user") appendUser(m.content);
      else if (m.card) appendCard(m.card);
      else if (m.content && m.content.trim()) appendAssistant(m.content, false);
    }
    scrollToBottom(true);
    onChanged();
  } catch (e) {
    /* missing/corrupt — start fresh */
    newChat();
  }
}

export function newChat() {
  if (streaming) return;
  currentId = null;
  clearMessages();
  showEmpty(true);
  onChanged();
  $("chatInput").focus();
}

function setStreaming(on) {
  streaming = on;
  $("chatInput").disabled = on;
  $("chatSend").disabled = on;
  $("chatStop").hidden = !on;
}

async function send(text) {
  const msg = (text || "").trim();
  if (!msg || streaming) return;
  showEmpty(false);
  appendUser(msg);
  $("chatInput").value = "";
  autoGrow();
  setStreaming(true);
  activeTurn = { assistantNode: null, pending: new Map() };
  try {
    const res = await call("chat_send", { conversation_id: currentId, message: msg });
    currentId = res.conversation_id || currentId;
  } catch (e) {
    const errText = e && e.message ? e.message : String(e);
    if (!activeTurn.assistantNode) appendAssistant("", true);
    const prose = activeTurn.assistantNode.querySelector(".prose");
    prose.dataset.raw = (prose.dataset.raw || "") + `\n\n⚠ ${errText}`;
  } finally {
    finalizeLive();
    setStreaming(false);
    onChanged();
  }
}

function autoGrow() {
  const ta = $("chatInput");
  ta.style.height = "auto";
  ta.style.height = Math.min(ta.scrollHeight, 200) + "px";
}

export function initChat(opts = {}) {
  onChanged = opts.onConversationChanged || (() => {});

  on("chat_delta", (e) => handleDelta(e.payload || {}));
  on("chat_tool", (e) => handleTool(e.payload || {}));
  on("chat_done", () => {
    /* send() resolution finalizes; nothing structural needed here */
  });
  // Backend dropped a fabricated free-form answer in favour of a real tool.
  on("chat_reset", () => {
    if (activeTurn && activeTurn.assistantNode) {
      const prose = activeTurn.assistantNode.querySelector(".prose");
      prose.dataset.raw = "";
      prose.textContent = "";
    }
  });
  // Legacy build-progress events surface on the active tool status, if any.
  on("build_progress", (e) => {
    const p = e && e.payload;
    if (!p || !p.detail || !activeTurn) return;
    const anyPending = [...activeTurn.pending.values()].flat()[0];
    if (anyPending) {
      const label = anyPending.querySelector(".tool-status-name");
      if (label) label.textContent = p.detail;
    }
  });

  const ta = $("chatInput");
  ta.addEventListener("input", autoGrow);
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      send(ta.value);
    }
  });
  // Webview drag-drop: a single .pdf primes the composer with an analyze command.
  on("tauri://drag-drop", (e) => {
    const paths = (e && e.payload && e.payload.paths) || [];
    const pdfs = paths.filter((p) => /\.pdf$/i.test(p));
    if (paths.length === 1 && pdfs.length === 1) {
      const path = pdfs[0];
      const stem = (path.split(/[\\/]/).pop() || "PDF").replace(/\.pdf$/i, "");
      ta.value = `Analyze the filing PDF at "${path}" for ${stem}`;
      autoGrow();
      ta.focus();
    } else if (paths.length) {
      const orig = ta.placeholder;
      ta.placeholder = "Drop one .pdf to analyze";
      setTimeout(() => {
        ta.placeholder = orig;
      }, 2500);
    }
  });
  $("chatSend").addEventListener("click", () => send(ta.value));
  $("chatStop").addEventListener("click", () => {
    call("chat_cancel", { conversation_id: currentId }).catch(() => {});
  });

  $("chatEmpty").addEventListener("click", (e) => {
    const chip = e.target.closest(".example-chip");
    if (chip) send(chip.textContent);
  });

  // Model pill opens Settings.
  const pill = $("modelPill");
  if (pill) pill.addEventListener("click", () => document.dispatchEvent(new CustomEvent("open-settings")));
}

// Reflect the current model in the composer pill.
export function setModelPill(model) {
  const pill = $("modelPillText");
  if (pill) pill.textContent = model || "no model";
}
