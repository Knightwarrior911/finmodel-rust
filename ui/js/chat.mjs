// chat.mjs — the chat pane: composer, streaming send flow, message rendering.

import { $, call, on, escapeHtml, renderMarkdown, stripControlTokens, copyToClipboard, flashBtn } from "./core.mjs";
import { renderCard } from "./cards.mjs";
import { closeReader } from "./reader.mjs";
import { openSettingsWithSkillDraft } from "./settings.mjs";

let currentId = null;
let streaming = false;
let onChanged = () => {};
let activeTurn = null; // { assistantNode, pending: Map<name, node[]> }
let activeRunId = null; // the current turn's run id (crypto.randomUUID)
export function getActiveRunId() {
  return activeRunId;
}
let pendingProjectId = null;
/// The next conversation created (on first send) is assigned to this project.
export function setPendingProjectId(id) {
  pendingProjectId = id || null;
}

/** RFC-4122-shaped id so backend validation accepts the same string Stop sends. */
/** Match backend `new_conversation` shape: `{ms}-{4hex}`. */
function newConversationId() {
  const ms = Date.now();
  const hex = Math.floor(Math.random() * 0x10000).toString(16).padStart(4, "0");
  return `${ms}-${hex}`;
}

function newRunId() {
  if (crypto.randomUUID) return crypto.randomUUID();
  // Fallback: 8-4-4-4-12 hex (version/variant bits not critical for format check).
  const h = () => Math.floor(Math.random() * 0x10000).toString(16).padStart(4, "0");
  return `${h()}${h()}-${h()}-4${h().slice(1)}-a${h().slice(1)}-${h()}${h()}${h()}`;
}

export function getCurrentId() {
  return currentId;
}

function scrollEl() {
  return $("chatScroll");
}

// Stick-to-bottom intent: true while the user is at/near the bottom. Driven by
// user scroll (below), NOT by post-growth position — so a fast-growing stream
// keeps following instead of disengaging after the first big chunk.
let stickToBottom = true;

function nearBottom() {
  const el = scrollEl();
  return el.scrollHeight - el.scrollTop - el.clientHeight < 48;
}

function scrollToBottom(force) {
  const el = scrollEl();
  if (force) stickToBottom = true;
  if (force || stickToBottom) el.scrollTop = el.scrollHeight;
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
  if (activeRunId) div.dataset.runId = activeRunId;
  if (currentId) div.dataset.conversationId = currentId;
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

function thinkIcon(name) {
  const svg = (p) =>
    `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">${p}</svg>`;
  switch (name) {
    case "get_financials":
    case "get_quote":
      return svg('<path d="M3 3v18h18"/><path d="M7 14l3-3 3 2 4-6"/>');
    case "benchmark_peers":
      return svg('<path d="M5 20V9M12 20V4M19 20v-7"/>');
    case "build_model":
      return svg('<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 9v12"/>');
    case "research":
    case "research_deal":
    case "web_search":
      return svg('<circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/>');
    case "read_page":
      return svg('<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 3 2.5 15 0 18M12 3c-2.5 3-2.5 15 0 18"/>');
    case "read_filing":
    case "list_filings":
    case "analyze_pdf":
      return svg('<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>');
    case "get_news":
      return svg('<path d="M4 5h12v14H5a1 1 0 0 1-1-1z"/><path d="M16 8h4v9a2 2 0 0 1-2 2M7 9h6M7 13h6"/>');
    case "use_skill":
      return svg('<path d="M5 4a2 2 0 0 1 2-2h11v18H7a2 2 0 0 0-2 2z"/>');
    default:
      return svg('<circle cx="12" cy="12" r="3.5"/>');
  }
}

// Lazily create the collapsible "Thinking process" panel for the active turn.
function ensureThinking() {
  if (!activeTurn) return null;
  if (activeTurn.thinkingNode) return activeTurn.thinkingNode;
  const panel = document.createElement("div");
  panel.className = "thinking msg";
  const toggle = document.createElement("button");
  toggle.type = "button";
  toggle.className = "thinking-toggle";
  toggle.setAttribute("aria-expanded", "true");
  toggle.innerHTML =
    `<span class="thinking-caret" aria-hidden="true">▾</span>` +
    `<span class="thinking-title">Thinking process</span>` +
    `<span class="thinking-count"></span>`;
  const steps = document.createElement("div");
  steps.className = "thinking-steps";
  toggle.addEventListener("click", () => {
    const collapsed = panel.classList.toggle("collapsed");
    toggle.setAttribute("aria-expanded", String(!collapsed));
  });
  panel.appendChild(toggle);
  panel.appendChild(steps);
  scrollEl().appendChild(panel);
  activeTurn.thinkingNode = panel;
  activeTurn.thinkingStepsEl = steps;
  activeTurn.stepCount = 0;
  scrollToBottom();
  return panel;
}

function updateThinkCount() {
  if (!activeTurn || !activeTurn.thinkingNode) return;
  const c = activeTurn.thinkingNode.querySelector(".thinking-count");
  const n = activeTurn.stepCount || 0;
  if (c) c.textContent = n ? `· ${n} step${n > 1 ? "s" : ""}` : "";
}

// Append a running tool step to the thinking panel; returns the step node.
function addThinkStep(name, detail) {
  ensureThinking();
  const step = document.createElement("div");
  step.className = "think-step running";
  step.innerHTML =
    `<span class="think-icon" aria-hidden="true">${thinkIcon(name)}</span>` +
    `<span class="think-label">${escapeHtml(phaseLabel(name, detail))}</span>` +
    `<span class="think-status"><span class="spinner"></span>In progress</span>`;
  activeTurn.thinkingStepsEl.appendChild(step);
  activeTurn.stepCount = (activeTurn.stepCount || 0) + 1;
  updateThinkCount();
  scrollToBottom();
  return step;
}

function finishThinkStep(step, ok) {
  if (!step) return;
  step.classList.remove("running");
  step.classList.add(ok ? "success" : "failed");
  const st = step.querySelector(".think-status");
  if (st)
    st.innerHTML = ok
      ? `<span class="think-tick" aria-hidden="true">✓</span>Success`
      : `<span class="think-x" aria-hidden="true">✗</span>Failed`;
}

// ── event stream wiring (correlated by conversation_id + run_id) ──
// Buffer text deltas and flush once per animation frame (Phase 3.5).
let deltaBuffer = "";
let deltaRaf = 0;

function eventMatchesActive(payload) {
  if (!activeTurn || !streaming) return false;
  if (payload.conversation_id && currentId && payload.conversation_id !== currentId) {
    return false;
  }
  if (payload.run_id && activeRunId && payload.run_id !== activeRunId) {
    return false;
  }
  // Missing ids: only accept while we have an active turn (legacy safety).
  return true;
}

function flushDeltaBuffer() {
  deltaRaf = 0;
  if (!activeTurn || !deltaBuffer) return;
  if (!activeTurn.assistantNode) {
    activeTurn.assistantNode = appendAssistant("", true);
  }
  const prose = activeTurn.assistantNode.querySelector(".prose");
  prose.dataset.raw = (prose.dataset.raw || "") + deltaBuffer;
  prose.textContent = prose.dataset.raw;
  deltaBuffer = "";
  scrollToBottom();
}

function handleDelta(payload) {
  if (!eventMatchesActive(payload)) return;
  deltaBuffer += payload.text || "";
  if (!deltaRaf) {
    deltaRaf = requestAnimationFrame(flushDeltaBuffer);
  }
}

/// Human phase label for the polite progress region.
function phaseLabel(name, detail) {
  if (detail && detail.trim()) return detail.trim();
  switch (name) {
    case "research":
      return "Researching…";
    case "analyze_pdf":
      return "Analyzing PDF…";
    case "build_model":
      return "Building model…";
    case "benchmark_peers":
      return "Benchmarking peers…";
    case "get_financials":
      return "Fetching financials…";
    case "get_quote":
      return "Fetching quote…";
    case "list_filings":
      return "Listing filings…";
    case "read_filing":
      return "Reading filing…";
    case "web_search":
      return "Searching the web…";
    case "read_page":
      return "Reading page…";
    case "research_deal":
      return "Researching deal…";
    case "get_news":
      return "Fetching news…";
    default:
      return `Running ${name}…`;
  }
}

/// Friendly progress label for an agent phase. Returns null for `executing`
/// (tool events drive the specific label there) and phases with no live status.
function agentPhaseLabel(phase) {
  switch (phase) {
    case "planning":
      return "Planning…";
    case "synthesizing":
      return "Writing the answer…";
    case "verifying":
      return "Checking the figures…";
    default:
      return null;
  }
}

function handleTool(payload) {
  if (!eventMatchesActive(payload)) return;
  const name = payload.name || "tool";
  if (payload.status === "fanout") {
    const n = payload.count || 2;
    ensureThinking();
    const note = document.createElement("div");
    note.className = "think-step note running";
    note.innerHTML =
      `<span class="think-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h11"/></svg></span>` +
      `<span class="think-label">Running ${n} tasks in parallel…</span>`;
    activeTurn.thinkingStepsEl.appendChild(note);
    activeTurn.fanoutNode = note;
    scrollToBottom();
    return;
  }
  if (payload.status === "fanout_done") {
    if (activeTurn && activeTurn.fanoutNode) {
      const n = payload.count || 2;
      activeTurn.fanoutNode.classList.remove("running");
      activeTurn.fanoutNode.innerHTML =
        `<span class="think-icon" aria-hidden="true"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"><path d="M4 7h16M4 12h16M4 17h11"/></svg></span>` +
        `<span class="think-label">${n} tasks ran in parallel</span>`;
      activeTurn.fanoutNode = null;
    }
    return;
  }
  if (payload.status === "start") {
    setProgress(phaseLabel(name, payload.detail));
    const step = addThinkStep(name, payload.detail);
    (activeTurn.toolSeq || (activeTurn.toolSeq = [])).push(name);
    const list = activeTurn.pending.get(name) || [];
    list.push(step);
    activeTurn.pending.set(name, list);
    return;
  }
  // Terminal status: mark the step and render the result card below the panel.
  setProgress("");
  const list = activeTurn.pending.get(name) || [];
  const step = list.shift();
  activeTurn.pending.set(name, list);
  if (payload.status === "error") {
    finishThinkStep(step, false);
    if (payload.card) {
      appendCard(payload.card);
    } else {
      const errNode = document.createElement("div");
      errNode.className = "msg msg-card";
      errNode.innerHTML = `<div class="card card-error"><p class="card-note err">${escapeHtml(
        payload.detail || (name + " failed")
      )}</p></div>`;
      scrollEl().appendChild(errNode);
      scrollToBottom();
    }
    return;
  }
  finishThinkStep(step, true);
  if (payload.card) appendCard(payload.card);
}

// After a multi-tool turn, offer to abstract it into a reusable skill
// (self-evolution): the model generalizes the transcript into a SKILL.md draft.
function addSkillAction(node, question, answer, tools) {
  if (node.querySelector(".msg-skill")) return;
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "msg-skill icon-btn";
  btn.textContent = "Save as skill";
  btn.title = "Abstract this into a reusable skill";
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    const prev = btn.textContent;
    btn.textContent = "Drafting…";
    const transcript =
      `User asked: ${question}\n\nTools used, in order: ${tools.join(", ")}.\n\nFinal answer:\n${answer}`;
    try {
      const draft = await call("skill_suggest", { transcript });
      openSettingsWithSkillDraft(typeof draft === "string" ? draft : "");
    } catch (_) {
      /* ignore draft failure */
    } finally {
      btn.textContent = prev;
      btn.disabled = false;
    }
  });
  node.appendChild(btn);
}

// Finalize the live assistant bubble: render its accumulated text as markdown.
function finalizeLive() {
  if (deltaRaf) {
    cancelAnimationFrame(deltaRaf);
    flushDeltaBuffer();
  }
  if (activeTurn && activeTurn.assistantNode) {
    activeTurn.assistantNode.classList.remove("streaming");
    const prose = activeTurn.assistantNode.querySelector(".prose");
    const clean = stripControlTokens(prose.dataset.raw || "");
    if (clean === "") {
      activeTurn.assistantNode.remove();
    } else {
      prose.innerHTML = renderMarkdown(clean);
      addCopyAction(activeTurn.assistantNode, clean);
      const tools = (activeTurn.toolSeq || []).filter((t) => t !== "use_skill");
      if (tools.length >= 2) addSkillAction(activeTurn.assistantNode, lastQuestion, clean, tools);
    }
  }
  if (activeTurn && activeTurn.thinkingNode) {
    activeTurn.thinkingNode.classList.add("collapsed");
    const tgl = activeTurn.thinkingNode.querySelector(".thinking-toggle");
    if (tgl) tgl.setAttribute("aria-expanded", "false");
  }
  activeTurn = null;
}

// ── public flows ────────────────────────────────────────────────────
export async function loadConversation(id) {
  if (streaming) return;
  try {
    const conv = await call("load_conversation", { id });
    currentId = conv.id;
    pendingProjectId = null;
    clearMessages();
    showEmpty(false);
    // Build history off-DOM then commit once (Phase 3.5).
    const frag = document.createDocumentFragment();
    const host = scrollEl();
    const appendTo = (node) => {
      frag.appendChild(node);
    };
    // Temporarily redirect appends into the fragment via a local host swap.
    const realAppend = host.appendChild.bind(host);
    host.appendChild = (n) => frag.appendChild(n);
    try {
      for (const m of conv.messages || []) {
        if (m.role === "user") appendUser(m.content);
        else if (m.card) appendCard(m.card);
        else if (m.content && m.content.trim()) appendAssistant(m.content, false);
      }
    } finally {
      host.appendChild = realAppend;
    }
    host.appendChild(frag);
    closeReader(); // changing chat resets the reader (Phase 4.3)
    scrollToBottom(true);
    onChanged();
  } catch (e) {
    // Load failure: announce, retain the current view, offer retry/close —
    // never silently discard into a new chat (Phase 4.3).
    const msg = (e && e.message) || "Couldn't open that conversation.";
    showLoadFailure(id, msg);
  }
}

/// Assertive announce with keyboard-reachable Retry / Dismiss for a failed
/// conversation load. Retains the current conversation and reader.
function showLoadFailure(id, message) {
  const a = $("chatAlert");
  if (!a) return;
  a.innerHTML = "";
  const note = document.createElement("span");
  note.className = "chat-alert-note";
  note.textContent = message;
  const retry = document.createElement("button");
  retry.type = "button";
  retry.className = "btn-ghost";
  retry.textContent = "Retry";
  retry.addEventListener("click", () => {
    clearAlert();
    loadConversation(id);
  });
  const dismiss = document.createElement("button");
  dismiss.type = "button";
  dismiss.className = "btn-ghost";
  dismiss.textContent = "Dismiss";
  dismiss.addEventListener("click", clearAlert);
  a.append(note, retry, dismiss);
  a.hidden = false;
  retry.focus();
}

export function newChat() {
  if (streaming) return;
  currentId = null;
  pendingProjectId = null;
  clearMessages();
  clearAlert();
  closeReader(); // new chat resets the reader (Phase 4.3)
  showEmpty(true);
  onChanged();
  $("chatInput").focus();
}

// Stop lifecycle: idle → running → stopping → terminal. First Stop shows
// "Stopping…" (disabled); repeats no-op; terminal clears aria-busy once.
let stopping = false;
let lastQuestion = "";

function setProgress(text) {
  const p = $("chatProgress");
  if (!p) return;
  if (text) {
    // Update only when the phase text actually changes (no duplicate announce).
    if (p.textContent !== text) p.textContent = text;
    p.hidden = false;
  } else {
    p.textContent = "";
    p.hidden = true;
  }
}

function clearAlert() {
  const a = $("chatAlert");
  if (a) {
    a.textContent = "";
    a.innerHTML = "";
    a.hidden = true;
  }
}

/// Terminal recovery region: preserve the question, offer Retry / New research.
function showRecovery(message) {
  const a = $("chatAlert");
  if (!a) return;
  a.innerHTML = "";
  const note = document.createElement("span");
  note.className = "chat-alert-note";
  note.textContent = message;
  const retry = document.createElement("button");
  retry.type = "button";
  retry.className = "btn-ghost";
  retry.textContent = "Retry";
  retry.addEventListener("click", () => {
    clearAlert();
    if (lastQuestion) send(lastQuestion);
  });
  const fresh = document.createElement("button");
  fresh.type = "button";
  fresh.className = "btn-ghost";
  fresh.textContent = "New research";
  fresh.addEventListener("click", () => {
    clearAlert();
    newChat();
  });
  a.append(note, retry, fresh);
  a.hidden = false;
}

function setStreaming(on) {
  streaming = on;
  $("chatInput").disabled = on;
  $("chatSend").disabled = on;
  const stop = $("chatStop");
  stop.hidden = !on;
  const scroll = $("chatScroll");
  if (on) {
    scroll.setAttribute("aria-busy", "true");
    // Reset Stop button to its active state for a new run.
    stopping = false;
    stop.disabled = false;
    stop.setAttribute("aria-label", "Stop");
  } else {
    // Terminal: clear aria-busy exactly once and hide progress.
    scroll.removeAttribute("aria-busy");
    setProgress("");
  }
}

/** Render an Approve/Deny group for a parked approval (risk-gated write). */
function renderApproval(env) {
  const runId = env.run_id;
  const payload = (env.event && env.event.payload) || {};
  const tcid = payload.tool_call_id || null;
  const box = document.createElement("div");
  box.className = "part-approval";
  box.setAttribute("role", "group");
  box.dataset.toolCallId = tcid || "";
  const label = document.createElement("span");
  label.className = "part-approval-text";
  label.textContent = "This action modifies or exports a file and needs your approval.";
  box.appendChild(label);
  const mk = (text, resp, cls) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = cls;
    b.textContent = text;
    b.addEventListener("click", () => {
      call("agent_approve", { run_id: runId, interaction_id: tcid, response: resp }).catch(() => {});
      box.remove();
    });
    return b;
  };
  box.appendChild(mk("Approve once", "approve_once", "btn-primary"));
  box.appendChild(mk("Create new version", "create_new_version", "btn-ghost"));
  box.appendChild(mk("Deny", "deny", "btn-ghost"));
  scrollEl().appendChild(box);
  scrollToBottom();
}

// Inline confirmation when the agent persists a manual "remember: X" save.
// The durable MemoryUpdated event carries the saved-row count.
function renderMemorySaved(count) {
  const box = document.createElement("div");
  box.className = "part-memory";
  const span = document.createElement("span");
  span.className = "part-memory-text";
  span.textContent = `Memory saved · ${count}`;
  box.appendChild(span);
  scrollEl().appendChild(box);
  scrollToBottom();
  // Fade out after a few seconds; the saved memory persists and influences
  // future turns via recall.
  setTimeout(() => {
    box.style.transition = "opacity .4s";
    box.style.opacity = "0";
    setTimeout(() => box.remove(), 400);
  }, 6000);
}

function agentEventKind(env) {
  return env && env.event && env.event.kind;
}

function waitForAgentTerminal(runId, timeoutMs = 130000) {
  return new Promise((resolve, reject) => {
    let unsub = () => {};
    const timer = setTimeout(() => {
      unsub();
      reject(new Error("agent turn timed out"));
    }, timeoutMs);
    const done = (result) => {
      clearTimeout(timer);
      unsub();
      resolve(result);
    };
    const p = on("agent_event", (e) => {
      const env = (e && e.payload) || {};
      if (env.run_id !== runId) return;
      const kind = agentEventKind(env);
      if (kind === "assistant_text_delta") {
        const chunk = env.event && env.event.payload && env.event.payload.text;
        if (chunk) handleDelta({ text: chunk, conversation_id: env.conversation_id, run_id: env.run_id });
        return;
      }
      if (kind === "approval_requested") {
        renderApproval(env);
        return;
      }
      if (kind === "phase_changed") {
        const label = agentPhaseLabel(env.event && env.event.payload && env.event.payload.phase);
        if (label) setProgress(label);
        return;
      }
      if (kind === "memory_updated") {
        const count = (env.event && env.event.payload && env.event.payload.count) || 0;
        if (count > 0) renderMemorySaved(count);
        return;
      }
      if (
        kind === "run_completed" ||
        kind === "run_failed" ||
        kind === "run_cancelled" ||
        kind === "run_interrupted" ||
        kind === "run_budget_limited"
      ) {
        done({ kind, env });
      }
    });
    // on() may return a Promise (Tauri 2) or an unsubscribe fn.
    if (p && typeof p.then === "function") {
      p.then((u) => {
        unsub = typeof u === "function" ? u : () => {};
      }).catch((err) => {
        clearTimeout(timer);
        reject(err);
      });
    } else if (typeof p === "function") {
      unsub = p;
    }
  });
}

async function sendViaAgent(msg) {
  const projectId = pendingProjectId;
  const res = await call("agent_send", {
    conversation_id: currentId || null,
    text: msg,
    project_id: projectId,
  });
  currentId = res.conversation_id || currentId;
  activeRunId = res.run_id || activeRunId;
  if (projectId) {
    pendingProjectId = null;
    onChanged(); // reflect the new chat under its folder in the sidebar
  }
  const terminal = await waitForAgentTerminal(activeRunId);
  // Surface a minimal final answer if no deltas streamed (fail-closed / error paths).
  if (!activeTurn.assistantNode) {
    const detail =
      (terminal.env &&
        terminal.env.event &&
        terminal.env.event.payload &&
        (terminal.env.event.payload.detail || terminal.env.event.payload.stop)) ||
      terminal.kind;
    appendAssistant(typeof detail === "string" ? detail : JSON.stringify(detail), true);
  }
}

async function send(text) {
  const msg = (text || "").trim();
  if (!msg || streaming) return;
  clearAlert();
  lastQuestion = msg;
  showEmpty(false);
  appendUser(msg);
  $("chatInput").value = "";
  autoGrow();
  setStreaming(true);
  activeTurn = { assistantNode: null, pending: new Map(), toolSeq: [] };
  activeRunId = newRunId();
  // Allocate conversation id before the long-running invoke so Stop can target
  // the registry key immediately (backend accepts client-supplied ids).
  if (!currentId) currentId = newConversationId();
  let cancelled = false;
  try {
    await sendViaAgent(msg);
    cancelled = stopping;
  } catch (e) {
    const errText = e && e.message ? e.message : String(e);
    if (!activeTurn.assistantNode) appendAssistant("", true);
    const prose = activeTurn.assistantNode.querySelector(".prose");
    prose.dataset.raw = (prose.dataset.raw || "") + `\n\n⚠ ${errText}`;
  } finally {
    finalizeLive();
    setStreaming(false);
    if (cancelled) {
      // Terminal cancel: preserve the question, offer Retry / New research.
      showRecovery("Stopped.");
    }
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
  // Track stick-to-bottom from user scroll. Programmatic scroll-to-bottom lands
  // near the bottom (flag stays true); scrolling up to read releases the follow;
  // scrolling back to the bottom re-engages it.
  scrollEl().addEventListener("scroll", () => { stickToBottom = nearBottom(); }, { passive: true });

  on("chat_delta", (e) => handleDelta(e.payload || {}));
  on("chat_tool", (e) => handleTool(e.payload || {}));
  on("chat_done", () => {
    /* send() resolution finalizes; nothing structural needed here */
  });
  // Backend dropped a fabricated free-form answer in favour of a real tool.
  on("chat_reset", (e) => {
    const p = (e && e.payload) || {};
    if (!eventMatchesActive(p)) return;
    deltaBuffer = "";
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
  // OS drag-drop: Rust observes paths and emits pdf_drop_ready. We claim
  // the one-use grant for the current conversation and put the opaque handle
  // in the composer — never the raw filesystem path.
  on("pdf_drop_ready", async () => {
    try {
      const convId = currentId;
      if (!convId) {
        const orig = ta.placeholder;
        ta.placeholder = "Open or start a chat, then drop the PDF again";
        setTimeout(() => {
          ta.placeholder = orig;
        }, 2500);
        return;
      }
      const res = await call("claim_dropped_pdf", { conversation_id: convId });
      if (!res || !res.artifact_id) {
        const orig = ta.placeholder;
        ta.placeholder = "Drop not claimed — try again";
        setTimeout(() => {
          ta.placeholder = orig;
        }, 2500);
        return;
      }
      const label = res.label || "PDF";
      ta.value = `Analyze PDF [${res.artifact_id}] for ${label}`;
      autoGrow();
      ta.focus();
    } catch (err) {
      const orig = ta.placeholder;
      ta.placeholder = (err && err.message) || "Drop failed";
      setTimeout(() => {
        ta.placeholder = orig;
      }, 2500);
    }
  });
  on("chat_progress", (e) => {
    const p = (e && e.payload) || {};
    if (!eventMatchesActive(p)) return;
    if (!stopping) setProgress(p.text || "");
  });
  $("chatSend").addEventListener("click", () => send(ta.value));
  $("chatStop").addEventListener("click", () => {
    if (stopping || !streaming) return; // repeats are a no-op
    stopping = true;
    const stop = $("chatStop");
    stop.disabled = true;
    stop.setAttribute("aria-label", "Stopping…");
    stop.title = "Stopping…";
    setProgress("Stopping…");
    call("agent_cancel", { conversation_id: currentId, run_id: activeRunId }).catch(() => {});
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

// Honest onboarding (Phase 4.6): distinguish capability states with the exact
// next action, and show demo tickers that actually work in the current state.
const DEMO_CHIPS = [
  "Build SAND.ST",
  "Benchmark SAND.ST, ASML.AS, NESN.SW",
  "Build NOVO-B.CO",
  "Build ATCO-B.ST",
];
const LIVE_CHIPS = [
  "Build NVDA with peers AMD, INTC, AVGO",
  "Read the risk factors in TSLA's latest 10-K",
  "Benchmark AAPL, MSFT, GOOGL — deck included",
  "Research NVDA data-center revenue growth",
];

export function applyCapability(settings) {
  const note = $("capabilityNote");
  const chips = $("exampleChips");
  if (!note || !chips || !settings) return;
  const hasKey = !!settings.has_key;
  const cap = settings.model_capability || null;
  const capForModel = cap && cap.model_id === settings.model ? cap : null;
  let text;
  let useLive = hasKey;
  if (!hasKey) {
    // No key: friendly demo-mode invitation, no jargon.
    text =
      "You're in demo mode with a few sample companies. Add your API key in Settings to analyze any company with live data and build real models.";
    useLive = false;
  } else if (capForModel && !capForModel.native_tools) {
    // Keyed but the model can't call tools — plain-language limitation + fix.
    text =
      "Your current AI model can chat but can't pull live data or build models. Choose a different model in Settings to unlock full analysis.";
  } else {
    // Ready: verified capable, or not-yet-checked (still usable).
    text = "Ready to analyze. Ask about any company, filing, or deal — or tap a starter below.";
  }
  note.textContent = text;
  const labels = useLive ? LIVE_CHIPS : DEMO_CHIPS;
  chips.innerHTML = labels
    .map((l) => `<button type="button" class="example-chip">${escapeHtml(l)}</button>`)
    .join("");
}
