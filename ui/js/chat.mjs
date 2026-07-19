// chat.mjs — the chat pane: composer, streaming send flow, message rendering.

import {
  $,
  call,
  on,
  escapeHtml,
  renderMarkdown,
  stripControlTokens,
  copyToClipboard,
  flashBtn,
} from "./core.mjs";
import { renderCard } from "./cards.mjs";
import {
  toolRunningLabel,
  toolDoneLabel,
  toolFailedLabel,
  fanoutRunningLabel,
  fanoutDoneLabel,
  agentPhaseLabel as phaseProgressLabel,
  verifyStatusLabel,
  missionMetaLine,
  approvalHead,
  approvalApproveLabel,
  approvalDenyLabel,
  approvalNewVersionLabel,
} from "./labels.mjs";
import { closeReader } from "./reader.mjs";
import { evidenceIngest, evidenceReset } from "./evidence.mjs";
import { scheduleDueLabel, draftOfferForCards } from "./labels.mjs";
import { openSettingsWithSkillDraft } from "./settings.mjs";

let currentId = null;
let turnCardTypes = new Set();
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
  const hex = Math.floor(Math.random() * 0x10000)
    .toString(16)
    .padStart(4, "0");
  return `${ms}-${hex}`;
}

function newRunId() {
  if (crypto.randomUUID) return crypto.randomUUID();
  // Fallback: 8-4-4-4-12 hex (version/variant bits not critical for format check).
  const h = () =>
    Math.floor(Math.random() * 0x10000)
      .toString(16)
      .padStart(4, "0");
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
  // Every card — live or replayed — also feeds the Evidence dock ledger.
  evidenceIngest(card);
  if (card && card.type) turnCardTypes.add(String(card.type));
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
      return svg(
        '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M3 9h18M9 9v12"/>',
      );
    case "research":
    case "research_deal":
    case "web_search":
      return svg('<circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/>');
    case "read_page":
      return svg(
        '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c2.5 3 2.5 15 0 18M12 3c-2.5 3-2.5 15 0 18"/>',
      );
    case "read_filing":
    case "list_filings":
    case "analyze_pdf":
      return svg(
        '<path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/>',
      );
    case "get_news":
      return svg(
        '<path d="M4 5h12v14H5a1 1 0 0 1-1-1z"/><path d="M16 8h4v9a2 2 0 0 1-2 2M7 9h6M7 13h6"/>',
      );
    case "use_skill":
      return svg('<path d="M5 4a2 2 0 0 1 2-2h11v18H7a2 2 0 0 0-2 2z"/>');
    default:
      return svg('<circle cx="12" cy="12" r="3.5"/>');
  }
}

// Lazily create the collapsible "Working through this" story for the active turn.
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
    `<span class="thinking-live" aria-hidden="true"><span class="thinking-pulse"></span></span>` +
    `<span class="thinking-caret" aria-hidden="true"><svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" stroke-linecap="round" stroke-linejoin="round"><path d="M6 9l6 6 6-6"/></svg></span>` +
    `<span class="thinking-title">Working through this</span>` +
    `<span class="thinking-meta" hidden></span>` +
    `<span class="thinking-count"></span>`;
  const status = document.createElement("div");
  status.className = "thinking-status";
  status.hidden = true;
  const steps = document.createElement("div");
  steps.className = "thinking-steps";
  toggle.addEventListener("click", () => {
    const collapsed = panel.classList.toggle("collapsed");
    toggle.setAttribute("aria-expanded", String(!collapsed));
  });
  panel.appendChild(toggle);
  panel.appendChild(status);
  panel.appendChild(steps);
  scrollEl().appendChild(panel);
  activeTurn.thinkingNode = panel;
  activeTurn.thinkingStepsEl = steps;
  activeTurn.stepCount = 0;
  activeTurn.mission = activeTurn.mission || {};
  renderThinkingMeta();
  const pendingStatus = activeTurn.pendingStatus || "";
  if (pendingStatus) setThinkingStatus(pendingStatus);
  scrollToBottom();
  return panel;
}

function setThinkingStatus(text) {
  if (!activeTurn) return;
  activeTurn.pendingStatus = text || "";
  if (!activeTurn.thinkingNode) {
    if (text) ensureThinking();
    return;
  }
  const el = activeTurn.thinkingNode.querySelector(".thinking-status");
  if (!el) return;
  if (text) {
    el.textContent = text;
    el.hidden = false;
  } else {
    el.textContent = "";
    el.hidden = true;
  }
}

function renderThinkingMeta() {
  if (!activeTurn || !activeTurn.thinkingNode) return;
  const meta = activeTurn.thinkingNode.querySelector(".thinking-meta");
  if (!meta) return;
  const line = missionMetaLine(activeTurn.mission || {});
  meta.textContent = line ? "· " + line : "";
  meta.hidden = !line;
}

function syncThinkingMission(patch) {
  if (!activeTurn) return;
  activeTurn.mission = { ...(activeTurn.mission || {}), ...patch };
  // Ensure the single status story exists once mission context arrives.
  if (patch.workflow || patch.planTotal || patch.verify || patch.phase) {
    ensureThinking();
  }
  renderThinkingMeta();
}

function updateThinkCount() {
  if (!activeTurn || !activeTurn.thinkingNode) return;
  const c = activeTurn.thinkingNode.querySelector(".thinking-count");
  const n = activeTurn.stepCount || 0;
  if (c) c.textContent = n ? `· ${n} check${n > 1 ? "s" : ""}` : "";
}

// Append a running tool step to the thinking panel; returns the step node.
function addThinkStep(name, detail) {
  ensureThinking();
  const step = document.createElement("div");
  step.className = "think-step running";
  step.dataset.t0 = String(performance.now());
  step.dataset.tool = String(name || "");
  step.dataset.detail = detail ? String(detail) : "";
  step.innerHTML =
    `<span class="think-icon" aria-hidden="true">${thinkIcon(name)}</span>` +
    `<span class="think-label">${escapeHtml(toolRunningLabel(name, detail))}</span>` +
    `<span class="think-status" role="status"><span class="think-dot" aria-hidden="true"></span><span class="sr-only">In progress</span></span>`;
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
  const name = step.dataset.tool || "";
  const detail = step.dataset.detail || "";
  const lab = step.querySelector(".think-label");
  if (lab) {
    lab.textContent = ok
      ? toolDoneLabel(name, detail)
      : toolFailedLabel(name);
  }
  const st = step.querySelector(".think-status");
  if (!st) return;
  // Soft outcome mark; keep duration quiet and secondary.
  const t0 = Number(step.dataset.t0 || 0);
  const secs = t0 ? (performance.now() - t0) / 1000 : 0;
  const dur = secs >= 1 ? `${secs.toFixed(0)}s` : "";
  st.innerHTML = ok
    ? `<span class="think-tick" aria-hidden="true"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round"><path d="M5 13l4 4L19 7"/></svg></span><span class="sr-only">Done</span>${dur ? `<span class="think-dur">${dur}</span>` : ""}`
    : `<span class="think-x" aria-hidden="true"><svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round"><path d="M6 6l12 12M18 6L6 18"/></svg></span><span class="sr-only">Couldn't finish</span>`;
}

// Render the mission plan (Task 2.3): the whole plan rides each `plan_updated`
// event, so re-render its steps with pending/running/done/blocked/skipped state.
// Additive to the live transcript — progress is event-driven, never time-based.
function renderPlan(plan) {
  if (!activeTurn) return;
  const steps = Array.isArray(plan && plan.steps) ? plan.steps : [];
  if (steps.length === 0) return;
  if (!activeTurn.planNode) {
    const panel = document.createElement("div");
    panel.className = "plan-panel msg";
    const head = document.createElement("div");
    head.className = "plan-head";
    const title = document.createElement("span");
    title.className = "plan-title";
    title.textContent = "What I'll do";
    const obj = document.createElement("span");
    obj.className = "plan-obj";
    head.append(title, obj);
    const ol = document.createElement("ol");
    ol.className = "plan-steps";
    panel.append(head, ol);
    scrollEl().appendChild(panel);
    activeTurn.planNode = panel;
  }
  const panel = activeTurn.planNode;
  if (plan.objective)
    panel.querySelector(".plan-obj").textContent = plan.objective;
  const ol = panel.querySelector(".plan-steps");
  ol.replaceChildren();
  let rovingSet = false;
  for (const s of steps) {
    const st = String((s && s.status) || "pending");
    const li = document.createElement("li");
    li.className = `plan-step status-${st}`;
    // Roving focus for ↑/↓ plan navigation (workbench.mjs). One tab stop.
    li.tabIndex = -1;
    if (st === "running") {
      li.setAttribute("aria-current", "step");
      li.tabIndex = 0;
      rovingSet = true;
    }
    const g = document.createElement("span");
    g.className = "plan-glyph";
    g.setAttribute("aria-hidden", "true");
    g.textContent =
      st === "done"
        ? "✓"
        : st === "running"
          ? "⟳"
          : st === "blocked"
            ? "✕"
            : st === "skipped"
              ? "–"
              : "○";
    const lab = document.createElement("span");
    lab.className = "plan-step-label";
    lab.textContent = (s && (s.label || s.id)) || "";
    li.append(g, lab);
    li.setAttribute("aria-label", `${lab.textContent}: ${st}`);
    ol.appendChild(li);
  }
  // Ensure exactly one tab stop even when no step is running.
  if (!rovingSet && ol.firstElementChild) ol.firstElementChild.tabIndex = 0;
  scrollToBottom();
}

// ── event stream wiring (correlated by conversation_id + run_id) ──
// Buffer text deltas and flush once per animation frame (Phase 3.5).
let deltaBuffer = "";
let deltaRaf = 0;

function eventMatchesActive(payload) {
  if (!activeTurn || !streaming) return false;
  if (
    payload.conversation_id &&
    currentId &&
    payload.conversation_id !== currentId
  ) {
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
  return toolRunningLabel(name, detail);
}

/// Friendly progress label for an agent phase. Returns null for `executing`
/// (tool events drive the specific label there) and phases with no live status.
function agentPhaseLabel(phase) {
  return phaseProgressLabel(phase);
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
      `<span class="think-label">${escapeHtml(fanoutRunningLabel(n))}</span>`;
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
        `<span class="think-label">${escapeHtml(fanoutDoneLabel(n))}</span>`;
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
  // Terminal status: mark the thinking step. The result CARD now rides the
  // durable `result_part_added` event (Task 2.1), not this transitional channel.
  setProgress("");
  const list = activeTurn.pending.get(name) || [];
  const step = list.shift();
  activeTurn.pending.set(name, list);
  finishThinkStep(step, payload.status !== "error");
}

// After a multi-tool turn, offer to abstract it into a reusable skill
// (self-evolution): the model generalizes the transcript into a SKILL.md draft.
function addSkillAction(node, question, answer, tools) {
  if (node.querySelector(".msg-skill")) return;
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "msg-skill icon-btn";
  btn.textContent = "Save as skill";
  btn.title = "Turn this into a reusable playbook";
  btn.addEventListener("click", async () => {
    btn.disabled = true;
    const prev = btn.textContent;
    btn.textContent = "Drafting…";
    const transcript = `User asked: ${question}\n\nTools used, in order: ${tools.join(", ")}.\n\nFinal answer:\n${answer}`;
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
      if (tools.length >= 2)
        addSkillAction(activeTurn.assistantNode, lastQuestion, clean, tools);
    }
  }
  if (activeTurn && activeTurn.thinkingNode) {
    activeTurn.thinkingNode.classList.add("collapsed", "thinking-done");
    const tgl = activeTurn.thinkingNode.querySelector(".thinking-toggle");
    if (tgl) tgl.setAttribute("aria-expanded", "false");
    const title = activeTurn.thinkingNode.querySelector(".thinking-title");
    if (title) title.textContent = "How I checked this";
    const live = activeTurn.thinkingNode.querySelector(".thinking-live");
    if (live) live.remove();
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
    evidenceReset(); // dock ledger rebuilds from the replayed cards below
    hideMission();
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
        else if (m.content && m.content.trim())
          appendAssistant(m.content, false);
      }
    } finally {
      host.appendChild = realAppend;
    }
    host.appendChild(frag);
    // A run the process died under (boot repair marked it 'interrupted')
    // stays resumable across reload — offer Resume just like a live pause.
    if (conv.last_run && conv.last_run.status === "interrupted") {
      activeRunId = conv.last_run.id;
      showResume(conv.last_run.id);
    }
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
  evidenceReset();
  hideMission();
  clearAlert();
  closeReader(); // new chat resets the reader (Phase 4.3)
  showEmpty(true);
  onChanged();
  $("chatInput").focus();
}

// Stop lifecycle: idle → running → stopping → terminal. First Stop shows
// "Stopping…" (disabled); repeats no-op; terminal clears aria-busy once.
let stopping = false;
let paused = false; // Pause was requested for the active run (resumable interrupt).
let lastTerminalKind = null; // Terminal EventKind of the just-finished run.
let lastQuestion = "";

function setProgress(text) {
  const p = $("chatProgress");
  if (!p) return;
  // Keep the polite live region for assistive tech, but fold the visible
  // status into the thinking story so mission/progress/thinking aren't stacked.
  p.classList.add("sr-only");
  if (text) {
    if (p.textContent !== text) p.textContent = text;
    p.hidden = false;
    if (activeTurn) setThinkingStatus(text);
  } else {
    p.textContent = "";
    p.hidden = true;
    if (activeTurn) setThinkingStatus("");
  }
}

// ── Mission status header (Task 2.2): workflow · phase · plan · verified ──
// Live, additive, driven by the same agent_event stream. `patch` sets any of
// { workflow, phase, planDone, planTotal, verify }.
function updateMission(patch) {
  const el = $("missionHeader");
  // Mission chrome stays in the DOM for compatibility, but is visually
  // collapsed — the thinking panel is the single calm status story.
  if (el) el.hidden = true;
  const thinkPatch = { ...patch };
  if ("workflow" in patch) {
    thinkPatch.workflow = missionWorkflowLabel(patch.workflow);
    if (el) {
      const w = el.querySelector(".mission-workflow");
      if (w) {
        w.textContent = thinkPatch.workflow || "";
        w.hidden = !thinkPatch.workflow;
      }
    }
  }
  if ("phase" in patch && el) {
    const p = el.querySelector(".mission-phase");
    if (p) {
      p.textContent = patch.phase || "";
      p.hidden = !patch.phase;
    }
  }
  if (("planDone" in patch || "planTotal" in patch) && el) {
    const pl = el.querySelector(".mission-plan");
    if (pl) {
      const total = patch.planTotal || 0;
      pl.textContent = total
        ? `${patch.planDone || 0} of ${total} done`
        : "";
      pl.hidden = !total;
    }
  }
  if ("verify" in patch && el) {
    const v = el.querySelector(".mission-verify");
    if (v) {
      const label = verifyStatusLabel(patch.verify);
      v.textContent = label
        ? `${patch.verify === "partial_unverified" ? "⚠" : "✓"} ${label}`
        : "";
      v.className = `mission-verify status-${patch.verify || ""}`;
      v.hidden = !patch.verify;
    }
  }
  syncThinkingMission(thinkPatch);
  if (patch.phase) setThinkingStatus(patch.phase);
}

function missionWorkflowLabel(id) {
  if (!id) return "";
  const names = {
    company_brief: "Company brief",
    earnings_review: "Earnings review",
    trading_comps: "Trading comps",
    dcf_model: "DCF model",
    ma_screen: "M&A screen",
    pitch_prep: "Pitch prep",
  };
  return names[id] || id;
}

function hideMission() {
  const el = $("missionHeader");
  if (el) {
    el.hidden = true;
    for (const span of el.querySelectorAll("span")) {
      span.textContent = "";
      span.hidden = true;
    }
    // Reset the verify span's status class too, so a prior run's badge colour never
    // lingers as a stale className after the text is cleared.
    const v = el.querySelector(".mission-verify");
    if (v) v.className = "mission-verify";
  }
  if (activeTurn) {
    activeTurn.mission = {};
    activeTurn.pendingStatus = "";
    renderThinkingMeta();
    setThinkingStatus("");
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

/// Paused (resumable-interrupt) recovery: offer Resume (relaunch the interrupted
/// run from its last complete boundary) or New research.
function showResume(interruptedRunId) {
  const a = $("chatAlert");
  if (!a) return;
  a.innerHTML = "";
  const note = document.createElement("span");
  note.className = "chat-alert-note";
  note.textContent = "Paused.";
  const resume = document.createElement("button");
  resume.type = "button";
  resume.className = "btn-ghost";
  resume.textContent = "Resume";
  resume.addEventListener("click", () => {
    clearAlert();
    resumeRun(interruptedRunId);
  });
  const fresh = document.createElement("button");
  fresh.type = "button";
  fresh.className = "btn-ghost";
  fresh.textContent = "New research";
  fresh.addEventListener("click", () => {
    clearAlert();
    newChat();
  });
  a.append(note, resume, fresh);
  a.hidden = false;
}

function setStreaming(on) {
  streaming = on;
  $("chatInput").disabled = on;
  $("chatSend").disabled = on;
  const stop = $("chatStop");
  stop.hidden = !on;
  const pause = $("chatPause");
  if (pause) pause.hidden = !on;
  const scroll = $("chatScroll");
  if (on) {
    scroll.setAttribute("aria-busy", "true");
    // Reset Stop button to its active state for a new run.
    stopping = false;
    paused = false;
    lastTerminalKind = null;
    stop.disabled = false;
    stop.setAttribute("aria-label", "Stop");
    if (pause) {
      pause.disabled = false;
      pause.setAttribute("aria-label", "Pause");
      pause.title = "Pause — you can resume later";
    }
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
  const risk = payload.risk || "";
  const box = document.createElement("div");
  box.className = "part-approval";
  box.setAttribute("role", "group");
  box.dataset.toolCallId = tcid || "";
  const head = document.createElement("div");
  head.className = "part-approval-head";
  head.textContent = approvalHead(risk);
  box.appendChild(head);
  if (payload.query || payload.target) {
    const target = document.createElement("div");
    target.className = "part-approval-target";
    target.textContent = payload.query || payload.target;
    box.appendChild(target);
  }
  const btns = document.createElement("div");
  btns.className = "part-approval-btns";
  const mk = (text, resp, cls) => {
    const b = document.createElement("button");
    b.type = "button";
    b.className = cls + " part-approval-btn";
    b.dataset.response = resp;
    b.textContent = text;
    b.addEventListener("click", () => {
      call("agent_approve", {
        run_id: runId,
        interaction_id: tcid,
        response: resp,
      }).catch(() => {});
      box.remove();
    });
    return b;
  };
  btns.appendChild(mk(approvalApproveLabel(), "approve_once", "btn-primary"));
  btns.appendChild(mk(approvalDenyLabel(), "deny", "btn-ghost"));
  if (risk === "local_overwrite" || risk === "export") {
    btns.appendChild(
      mk(approvalNewVersionLabel(), "create_new_version", "btn-ghost"),
    );
  }
  box.appendChild(btns);
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
  span.textContent =
    count === 1
      ? "Got it — I'll remember that."
      : `Got it — remembered ${count} notes.`;
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
        if (chunk)
          handleDelta({
            text: chunk,
            conversation_id: env.conversation_id,
            run_id: env.run_id,
          });
        return;
      }
      if (kind === "approval_requested") {
        renderApproval(env);
        return;
      }
      if (kind === "phase_changed") {
        const label = agentPhaseLabel(
          env.event && env.event.payload && env.event.payload.phase,
        );
        if (label) {
          setProgress(label);
          updateMission({ phase: label });
        }
        return;
      }
      if (kind === "plan_updated") {
        const plan = (env.event && env.event.payload) || {};
        renderPlan(plan);
        const steps = Array.isArray(plan.steps) ? plan.steps : [];
        updateMission({
          workflow: plan.workflow && plan.workflow.id,
          planDone: steps.filter((s) => s && s.status === "done").length,
          planTotal: steps.length,
        });
        return;
      }
      if (kind === "result_part_added") {
        const card = env.event && env.event.payload && env.event.payload.card;
        if (card && typeof card === "object") {
          appendCard(card);
          if (card.type === "verification" && card.status) {
            updateMission({ verify: card.status });
          }
        }
        return;
      }
      if (kind === "tool_started") {
        const p = (env.event && env.event.payload) || {};
        const step = addThinkStep(p.name || "tool");
        setProgress(phaseLabel(p.name || "tool"));
        if (!activeTurn.thinkById) activeTurn.thinkById = new Map();
        if (p.tool_call_id) activeTurn.thinkById.set(p.tool_call_id, step);
        return;
      }
      if (kind === "tool_succeeded" || kind === "tool_failed") {
        const p = (env.event && env.event.payload) || {};
        const step =
          activeTurn.thinkById && activeTurn.thinkById.get(p.tool_call_id);
        finishThinkStep(step, kind === "tool_succeeded");
        if (activeTurn.thinkById) activeTurn.thinkById.delete(p.tool_call_id);
        setProgress("");
        return;
      }
      if (kind === "memory_updated") {
        const count =
          (env.event && env.event.payload && env.event.payload.count) || 0;
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
        const terminalPhase = {
          run_completed: "Delivered",
          run_failed: "Failed",
          run_cancelled: "Stopped",
          run_interrupted: "Paused",
          run_budget_limited: "Budget reached",
        };
        updateMission({ phase: terminalPhase[kind] || "" });
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
  turnCardTypes = new Set();
  const res = await call("agent_send", {
    conversation_id: currentId || null,
    text: msg,
    project_id: projectId,
  });
  currentId = res.conversation_id || currentId;
  activeRunId = res.run_id || activeRunId;
  // A follow-up promise in the message becomes a PROPOSAL — scheduling always
  // needs an explicit yes (never silently created from inference).
  if (res.commitment && res.commitment.text) renderScheduleOffer(res.commitment);
  // A standing preference becomes a proposal too — remembered only on yes.
  else if (res.memory_candidate && res.memory_candidate.text)
    renderMemoryOffer(res.memory_candidate);
  if (projectId) {
    pendingProjectId = null;
    onChanged(); // reflect the new chat under its folder in the sidebar
  }
  const terminal = await waitForAgentTerminal(activeRunId);
  lastTerminalKind = terminal.kind;
  surfaceMinimalAnswer(terminal);
  // Evidence gathered, no memo yet, run finished clean, nothing else being
  // offered → quietly suggest the write-up (drafting needs an explicit yes).
  if (
    terminal.kind === "run_completed" &&
    !(res.commitment && res.commitment.text) &&
    !(res.memory_candidate && res.memory_candidate.text)
  ) {
    const offer = draftOfferForCards([...turnCardTypes]);
    if (offer) renderDraftOffer(offer);
  }
}

/// Quiet, dismissible offer to remember a standing preference the user just
/// stated ("always show figures in USD millions"). Saving requires the
/// explicit yes — the unattended classifier stays off (precision doctrine).
function renderMemoryOffer(candidate) {
  const div = document.createElement("div");
  div.className = "msg schedule-offer memory-offer";
  div.innerHTML =
    `<span class="schedule-offer-text">That sounds like a standing preference — want me to remember it for future work?</span>` +
    `<span class="schedule-offer-actions">` +
    `<button type="button" class="btn-primary memory-yes">Remember it</button>` +
    `<button type="button" class="btn-ghost memory-no">No thanks</button>` +
    `</span>`;
  div.querySelector(".memory-yes").addEventListener("click", async () => {
    try {
      await call("memory_add", { content: candidate.text });
      div.innerHTML = `<span class="schedule-offer-text">Remembered — you can review or remove it in Settings → Memory.</span>`;
    } catch (e) {
      div.innerHTML = `<span class="schedule-offer-text">Couldn't save that${e && e.message ? ` (${escapeHtml(e.message)})` : ""} — you can add it from Settings → Memory.</span>`;
    }
  });
  div.querySelector(".memory-no").addEventListener("click", () => div.remove());
  scrollEl().appendChild(div);
  scrollToBottom();
}

/// Quiet, dismissible offer to draft the memo after an evidence-gathering
/// turn. Drafting runs only on the explicit yes - the click sends the same
/// chat message the user could have typed.
function renderDraftOffer(offer) {
  if (document.querySelector(".draft-offer")) return;
  const div = document.createElement("div");
  div.className = "msg schedule-offer draft-offer";
  div.innerHTML =
    `<span class="schedule-offer-text">${escapeHtml(offer.text)}</span>` +
    `<span class="schedule-offer-actions">` +
    `<button type="button" class="btn-primary draft-yes">${escapeHtml(offer.prompt)}</button>` +
    `<button type="button" class="btn-ghost draft-no">No thanks</button>` +
    `</span>`;
  div.querySelector(".draft-yes").addEventListener("click", () => {
    div.remove();
    send(offer.prompt);
  });
  div.querySelector(".draft-no").addEventListener("click", () => div.remove());
  scrollEl().appendChild(div);
  scrollToBottom();
}

/// Quiet, dismissible offer to schedule a follow-up the user promised in
/// their own words ("re-run this after earnings"). Approving creates the
/// schedule; the 60-second tick launches it when due.
function renderScheduleOffer(commitment) {
  const when = scheduleDueLabel(commitment.due);
  const div = document.createElement("div");
  div.className = "msg schedule-offer";
  div.innerHTML =
    `<span class="schedule-offer-text">Want me to come back to this ${escapeHtml(when)}? I'll re-run it and drop the update in this chat.</span>` +
    `<span class="schedule-offer-actions">` +
    `<button type="button" class="btn-primary schedule-yes">Schedule it</button>` +
    `<button type="button" class="btn-ghost schedule-no">No thanks</button>` +
    `</span>`;
  div.querySelector(".schedule-yes").addEventListener("click", async () => {
    try {
      await call("schedule_create", {
        conversation_id: currentId,
        prompt: commitment.text,
        due: commitment.due || null,
        recurrence: null,
      });
      div.innerHTML = `<span class="schedule-offer-text">Scheduled — I'll pick this up ${escapeHtml(when)}.</span>`;
    } catch (e) {
      div.innerHTML = `<span class="schedule-offer-text">Couldn't save that schedule${e && e.message ? ` (${escapeHtml(e.message)})` : ""} — try again in a moment.</span>`;
    }
  });
  div.querySelector(".schedule-no").addEventListener("click", () => div.remove());
  scrollEl().appendChild(div);
  scrollToBottom();
}

/// If no live delta bubble was created (fail-closed / non-streaming terminal),
/// surface a minimal final answer so the turn never renders empty. Always human
/// prose — never raw payload JSON (a budget stop once leaked
/// '{"detail":"rounds","kind":"budget"}' into the chat).
function surfaceMinimalAnswer(terminal) {
  if (activeTurn && activeTurn.assistantNode) return;
  const payload =
    (terminal.env && terminal.env.event && terminal.env.event.payload) || {};
  const stop = payload.stop;
  let text;
  switch (terminal.kind) {
    case "run_budget_limited": {
      const which =
        stop && stop.detail === "tokens"
          ? "token budget"
          : stop && stop.detail === "deadline"
            ? "time budget"
            : "step budget";
      text = `I hit this turn's ${which} before I could finish. Ask me to continue and I'll pick up from what I found so far.`;
      break;
    }
    case "run_cancelled":
      text = "Stopped at your request.";
      break;
    case "run_interrupted":
      text = "Paused — use Resume to continue this run.";
      break;
    case "run_failed": {
      const code =
        typeof stop === "object" && stop && typeof stop.detail === "string"
          ? ` (${stop.detail})`
          : "";
      text = `Something went wrong and this run could not finish${code}. Try again, or rephrase the request.`;
      break;
    }
    default:
      text =
        typeof payload.detail === "string" ? payload.detail : "Done.";
  }
  appendAssistant(text, true);
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
  hideMission();
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
    } else if (paused && lastTerminalKind === "run_interrupted") {
      // Resumable pause: offer Resume (relaunch from the last complete boundary).
      showResume(activeRunId);
    }
    onChanged();
  }
}

/// Relaunch an interrupted (paused) run via `agent_resume`; the backend seeds a
/// new linked run from the last complete boundary and drives it to terminal.
async function resumeRun(interruptedRunId) {
  if (streaming || !interruptedRunId) return;
  clearAlert();
  showEmpty(false);
  setStreaming(true);
  activeTurn = { assistantNode: null, pending: new Map(), toolSeq: [] };
  hideMission();
  let cancelled = false;
  try {
    const newRid = await call("agent_resume", {
      interrupted_run_id: interruptedRunId,
    });
    activeRunId =
      (typeof newRid === "string" ? newRid : newRid && newRid.run_id) ||
      activeRunId;
    const terminal = await waitForAgentTerminal(activeRunId);
    lastTerminalKind = terminal.kind;
    surfaceMinimalAnswer(terminal);
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
      showRecovery("Stopped.");
    } else if (paused && lastTerminalKind === "run_interrupted") {
      showResume(activeRunId);
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
  scrollEl().addEventListener(
    "scroll",
    () => {
      stickToBottom = nearBottom();
    },
    { passive: true },
  );

  // Text deltas + tool status now arrive on the single `agent_event` channel
  // (Task 2.1): assistant_text_delta / tool_started / tool_succeeded /
  // tool_failed / result_part_added, handled in waitForAgentTerminal.
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
        ta.placeholder = "Couldn't attach that PDF — try dropping it again";
        setTimeout(() => {
          ta.placeholder = orig;
        }, 2500);
        return;
      }
      const label = res.label || "PDF";
      ta.value = `Analyze the PDF “${label}”`;
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
    call("agent_cancel", {
      conversation_id: currentId,
      run_id: activeRunId,
    }).catch(() => {});
  });

  const pauseBtn = $("chatPause");
  if (pauseBtn) {
    pauseBtn.addEventListener("click", () => {
      if (paused || stopping || !streaming) return; // one pause per run; not after Stop
      paused = true;
      pauseBtn.disabled = true;
      pauseBtn.setAttribute("aria-label", "Pausing…");
      pauseBtn.title = "Pausing…";
      setProgress("Pausing…");
      call("agent_pause", {
        conversation_id: currentId,
        run_id: activeRunId,
      }).catch(() => {});
    });
  }

  $("chatEmpty").addEventListener("click", (e) => {
    const chip = e.target.closest(".example-chip");
    if (chip) send(chip.textContent);
  });

  // Model pill opens Settings.
  const pill = $("modelPill");
  if (pill)
    pill.addEventListener("click", () =>
      document.dispatchEvent(new CustomEvent("open-settings")),
    );
}

// Reflect the current model in the composer pill.
export function setModelPill(model) {
  const pill = $("modelPillText");
  if (pill) pill.textContent = model || "Choose a model";
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
    text =
      "Ready to analyze. Ask about any company, filing, or deal — or tap a starter below.";
  }
  note.textContent = text;
  const labels = useLive ? LIVE_CHIPS : DEMO_CHIPS;
  chips.innerHTML = labels
    .map(
      (l) =>
        `<button type="button" class="example-chip">${escapeHtml(l)}</button>`,
    )
    .join("");
}
