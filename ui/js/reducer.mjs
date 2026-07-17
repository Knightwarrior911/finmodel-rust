// reducer.mjs — Conversation state reducer (Phase D).
//
// Pure state reduction for AgentEventEnvelope events. Manages conversation
// state, active runs, streaming status, message parts, and view state.
// Renders nothing — callers subscribe to state changes and update the DOM.
//
// Usage:
//   import { createStore, reduce } from "./reducer.mjs";
//   let state = createStore();
//   state = reduce(state, envelope); // returns new state (immutable)

// ── State shape ───────────────────────────────────────────────────────

/**
 * @typedef {Object} Message
 * @property {string} id
 * @property {"user"|"assistant"} role
 * @property {string} text
 * @property {boolean} streaming
 * @property {boolean} verified
 * @property {Array<string>} sources
 * @property {Array<string>} errors
 */

/**
 * @typedef {Object} ConversationState
 * @property {string|null} conversationId
 * @property {string|null} activeRunId
 * @property {boolean} streaming
 * @property {boolean} stopping
 * @property {string|null} lastQuestion
 * @property {Array<Message>} messages
 * @property {string} draftText  - in-progress assistant text
 * @property {string|null} phaseLabel
 * @property {string|null} lastAnnounce
 * @property {string|null} runStatus  - "running"|"completed"|"failed"|"cancelled"|"interrupted"|null
 * @property {Object|null} approvalRequest  - pending approval
 */

/** Create a fresh initial state. */
export function createStore() {
  return {
    conversationId: null,
    activeRunId: null,
    streaming: false,
    stopping: false,
    lastQuestion: null,
    messages: [],
    draftText: "",
    phaseLabel: null,
    lastAnnounce: null,
    runStatus: null,
    approvalRequest: null,
  };
}

// ── Reducer ───────────────────────────────────────────────────────────

/**
 * Process an AgentEventEnvelope and return a new state (shallow copy
 * with the relevant fields updated). Pure function — does not mutate `prev`.
 *
 * @param {ConversationState} prev
 * @param {Object} envelope - { event: { type, ... }, conversation_id, run_id, durability, ... }
 * @returns {ConversationState}
 */
export function reduce(prev, envelope) {
  const ev = envelope.event || {};
  const type = ev.type;
  const runId = envelope.run_id || prev.activeRunId;
  const convId = envelope.conversation_id || prev.conversationId;

  // Helper: create new state with one field updated.
  const set = (fields) => ({ ...prev, ...fields });

  switch (type) {
    case "RunStarted":
    case "run_started":
      return set({
        activeRunId: runId,
        conversationId: convId,
        streaming: true,
        stopping: false,
        draftText: "",
        runStatus: "running",
        phaseLabel: "Preparing…",
        approvalRequest: null,
      });

    case "PhaseChanged":
    case "phase_changed":
      return set({
        phaseLabel: ev.phase || ev.detail || prev.phaseLabel,
      });

    case "PlanUpdated":
    case "plan_updated":
    case "plan":
      return set({
        phaseLabel: ev.detail || ev.text || "Planning…",
      });

    case "AssistantTextDelta":
    case "assistant_text_delta":
    case "chat_delta": {
      const text = (prev.draftText || "") + (ev.text || "");
      return set({ draftText: text });
    }

    case "AssistantCheckpoint":
    case "assistant_checkpoint":
    case "chat_done": {
      const finalText = ev.text || prev.draftText;
      const msg = makeMessage(prev.conversationId, "assistant", finalText, false);
      return set({
        messages: [...prev.messages, msg],
        draftText: "",
      });
    }

    case "UserMessage":
    case "user_message": {
      const msg = makeMessage(convId, "user", ev.text || ev.detail || "", false);
      return set({
        messages: [...prev.messages, msg],
        lastQuestion: ev.text || ev.detail || prev.lastQuestion,
      });
    }

    case "ToolQueued":
    case "tool_queued":
    case "ToolStarted":
    case "tool_started":
      return set({ phaseLabel: ev.label || ev.name || "Running tool…" });

    case "ToolSucceeded":
    case "tool_succeeded":
      return set({ phaseLabel: null });

    case "ToolFailed":
    case "tool_failed":
      return set({ phaseLabel: null });

    case "ToolWarning":
    case "tool_warning":
      return set({ phaseLabel: ev.detail || "Warning" });

    case "ToolCancelled":
    case "tool_cancelled":
      return set({ phaseLabel: null });

    case "ApprovalRequested":
    case "approval_requested":
      return set({
        approvalRequest: {
          tool_call_id: ev.tool_call_id,
          name: ev.name,
          query: ev.query || ev.detail,
          risk: ev.risk,
        },
        phaseLabel: "Awaiting approval…",
      });

    case "ApprovalResolved":
    case "approval_resolved":
      return set({
        approvalRequest: null,
        phaseLabel: ev.response === "deny" ? "Denied" : "Approved",
      });

    case "MemoryUpdated":
    case "memory_updated":
      return set({ lastAnnounce: ev.count ? `Memory updated · ${ev.count}` : "Memory updated" });

    case "RunCompleted":
    case "run_completed":
      return set({
        streaming: false,
        runStatus: "completed",
        phaseLabel: null,
        stopping: false,
      });

    case "RunFailed":
    case "run_failed":
      return set({
        streaming: false,
        runStatus: "failed",
        phaseLabel: ev.error || "Failed",
        stopping: false,
      });

    case "RunCancelled":
    case "run_cancelled":
      return set({
        streaming: false,
        runStatus: "cancelled",
        phaseLabel: null,
        stopping: false,
      });

    case "RunInterrupted":
    case "run_interrupted":
      return set({
        streaming: false,
        runStatus: "interrupted",
        phaseLabel: null,
        stopping: false,
      });

    case "RunBudgetLimited":
    case "run_budget_limited":
      return set({
        streaming: false,
        runStatus: "budget_limited",
        phaseLabel: "Budget limit reached",
        stopping: false,
      });

    case "Error":
    case "error":
      return set({
        lastAnnounce: ev.detail || ev.error || "An error occurred",
      });

    case "StopRequested":
    case "stop_requested":
      return set({ stopping: true, phaseLabel: "Stopping…" });

    case "StopComplete":
    case "stop_complete":
      return set({ stopping: false, streaming: false, runStatus: "cancelled" });

    default:
      // Unknown event types pass through unchanged.
      return prev;
  }
}

/** Create a message object. */
function makeMessage(convId, role, text, streaming) {
  return {
    id: `${convId || "conv"}_msg_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
    role,
    text,
    streaming,
    verified: false,
    sources: [],
    errors: [],
  };
}

/** Update a message's text (for streaming updates). */
export function updateDraft(state, text) {
  return { ...state, draftText: text };
}

/** Set conversation ID without resetting the run. */
export function setConversation(state, id) {
  return { ...state, conversationId: id, messages: [], draftText: "" };
}

/** Mark the last assistant message as verified (add verification badge). */
export function markVerified(state) {
  const msgs = [...state.messages];
  for (let i = msgs.length - 1; i >= 0; i--) {
    if (msgs[i].role === "assistant") {
      msgs[i] = { ...msgs[i], verified: true };
      break;
    }
  }
  return { ...state, messages: msgs };
}
