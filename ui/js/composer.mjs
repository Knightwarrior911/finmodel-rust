// Composer input surface: attachments (picker / paste / drag-drop) and the
// in-composer model picker with type-ahead filtering.
//
// Pure logic (filtering, paste classification, size formatting) is exported
// for tests; DOM wiring happens in initComposer(). The backend never sees a
// raw filesystem path from here — bytes go up as base64 and come back as
// opaque artifact handles scoped to an owner token.

import { call, on } from "./core.mjs";

// ── pure helpers ────────────────────────────────────────────────────

/** Case-insensitive model filter over id and display name; empty query = all.
 * Matches every whitespace-separated term (type "claude son" → sonnet). */
export function filterModels(models, query) {
  const q = String(query || "").trim().toLowerCase();
  if (!q) return models.slice();
  const terms = q.split(/\s+/);
  return models.filter((m) => {
    const hay = `${m.id || ""} ${m.name || ""}`.toLowerCase();
    return terms.every((t) => hay.includes(t));
  });
}

/** Human size: 843 B, 12.4 KB, 3.1 MB. */
export function formatSize(bytes) {
  const n = Number(bytes) || 0;
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / (1024 * 1024)).toFixed(1)} MB`;
}

/** Paste classification: what should a paste of `text` become?
 * - "url": a lone http(s) URL → leave in the box, hint that it'll be read
 * - "attach": very long text → convert to a text attachment
 * - "text": ordinary paste                                             */
export function classifyPaste(text) {
  const t = String(text || "").trim();
  if (/^https?:\/\/\S+$/i.test(t)) return "url";
  if (t.length > 6000) return "attach";
  return "text";
}

/** Price per million prompt tokens, for the picker row. */
export function modelPriceLabel(m) {
  const p = m && m.pricing && (m.pricing.prompt ?? m.pricing.input);
  const v = Number(p);
  if (!Number.isFinite(v) || v <= 0) return "";
  return `$${(v * 1e6).toFixed(2)}/M`;
}

/** Context length label: 128000 → "128k ctx". */
export function contextLabel(m) {
  const c = Number(m && m.context_length);
  if (!Number.isFinite(c) || c <= 0) return "";
  return c >= 1000 ? `${Math.round(c / 1000)}k ctx` : `${c} ctx`;
}

const IMAGE_EXT = /\.(png|jpe?g|gif|webp)$/i;
export function isImageName(name) {
  return IMAGE_EXT.test(String(name || ""));
}

// ── composer state ──────────────────────────────────────────────────

const state = {
  attachments: [], // {artifact_id, scope, name, class, size, thumbUrl?}
  scope: null, // owner token echoed to stage/claim/send
  getConversationId: () => null,
};

function newScope() {
  return `stg-${Math.random().toString(36).slice(2, 10)}${Date.now().toString(36)}`;
}

/** Owner token for staging: the open conversation, else a sticky staging
 * token that the send path echoes back so the backend can resolve. */
export function attachScope() {
  const conv = state.getConversationId();
  if (conv) return conv;
  if (!state.scope) state.scope = newScope();
  return state.scope;
}

export function getAttachments() {
  return state.attachments.slice();
}

/** Payload for agent_send. */
export function attachmentPayload() {
  return state.attachments.map((a) => ({
    artifact_id: a.artifact_id,
    scope: a.scope,
    name: a.name,
    class: a.class,
  }));
}

/// Monotonic suffix so two screenshots pasted in the same second never
/// collide on name.
let screenshotSeq = 0;
export function clearAttachments() {
  state.attachments = [];
  state.scope = null;
  renderChips();
}

function removeAttachment(id) {
  const i = state.attachments.findIndex((a) => a.artifact_id === id);
  if (i >= 0) {
    state.attachments.splice(i, 1);
    renderChips();
  }
}

// ── chips rendering ─────────────────────────────────────────────────

const CLASS_LABEL = {
  pdf: "PDF",
  image: "Image",
  sheet: "Workbook",
  deck: "Deck",
  doc: "Document",
  text: "Text",
};

function esc(s) {
  return String(s ?? "").replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c],
  );
}

function renderChips() {
  const row = document.getElementById("composerAttachments");
  if (!row) return;
  if (!state.attachments.length) {
    row.innerHTML = "";
    row.hidden = true;
    return;
  }
  row.hidden = false;
  row.innerHTML = state.attachments
    .map(
      (a) => `<span class="attach-chip" data-att="${esc(a.artifact_id)}">
        ${a.thumbUrl ? `<img class="attach-thumb" alt="" src="${esc(a.thumbUrl)}">` : `<span class="attach-kind">${esc(CLASS_LABEL[a.class] || "File")}</span>`}
        <span class="attach-name" title="${esc(a.name)}">${esc(a.name)}</span>
        <span class="attach-size">${esc(formatSize(a.size))}</span>
        <button type="button" class="attach-remove" aria-label="Remove ${esc(a.name)}">×</button>
      </span>`,
    )
    .join("");
}

function composerHint(text, ms = 2600, action = null) {
  const el = document.getElementById("composerHint");
  if (!el) return;
  el.textContent = text;
  if (action) {
    const b = document.createElement("button");
    b.type = "button";
    b.className = "hint-action";
    b.textContent = action.label;
    b.addEventListener("click", () => {
      el.hidden = true;
      action.onClick();
    });
    el.append(" ", b);
  }
  el.hidden = false;
  clearTimeout(composerHint._t);
  composerHint._t = setTimeout(() => {
    el.hidden = true;
  }, ms);
}

// ── prompt polish (never auto-sends) ────────────────────────────────

let refineBusy = false;

/// Rewrite the draft in the input box. `mode`:
/// - "tidy"  — light cleanup (grammar, specifics the draft implies)
/// - "power" — full prompt-engineering pass: goal, grounding, output shape
/// The rewrite lands back in the box for the user to edit or undo —
/// sending stays a human decision.
export async function refineDraft(mode = "tidy") {
  const ta = document.getElementById("chatInput");
  const btn = document.getElementById("refineBtn");
  const draft = ((ta && ta.value) || "").trim();
  if (!draft) {
    composerHint("Type a question first, then I can tidy it up.");
    return;
  }
  if (refineBusy) return;
  refineBusy = true;
  if (btn) {
    btn.disabled = true;
    btn.classList.add("refining");
  }
  composerHint(
    mode === "power"
      ? "Building the strongest version of your request…"
      : "Tidying your question…",
    60_000,
  );
  try {
    const res = await call("refine_prompt", { draft, mode });
    const text = ((res && res.text) || "").trim();
    if (!text) throw new Error("Nothing came back — your draft is unchanged.");
    const original = ta.value;
    const Ev = ta.ownerDocument.defaultView.Event;
    ta.value = text;
    ta.dispatchEvent(new Ev("input", { bubbles: true }));
    ta.focus();
    composerHint(
      mode === "power" ? "Power prompt ready — send it, edit it, or" : "Tidied up — send it, edit it, or",
      8_000,
      {
      label: "put my version back",
      onClick: () => {
        ta.value = original;
        ta.dispatchEvent(new Ev("input", { bubbles: true }));
        ta.focus();
      },
    });
  } catch (e) {
    composerHint((e && e.message) || "Couldn't polish that just now.");
  } finally {
    refineBusy = false;
    if (btn) {
      btn.disabled = false;
      btn.classList.remove("refining");
    }
  }
}

// ── staging ─────────────────────────────────────────────────────────

async function fileToBase64(file) {
  const buf = await file.arrayBuffer();
  const bytes = new Uint8Array(buf);
  let bin = "";
  const CHUNK = 0x8000;
  for (let i = 0; i < bytes.length; i += CHUNK) {
    bin += String.fromCharCode(...bytes.subarray(i, i + CHUNK));
  }
  return btoa(bin);
}

/** Stage one File/Blob; adds a chip on success. */
export async function stageFile(file, nameOverride) {
  const name = nameOverride || file.name || "attachment";
  try {
    const b64 = await fileToBase64(file);
    const scope = attachScope();
    const res = await call("stage_attachment", {
      owner: scope,
      name,
      bytesB64: b64,
    });
    const att = {
      artifact_id: res.artifact_id,
      scope,
      name: res.label || name,
      class: res.class,
      size: res.size ?? file.size,
    };
    if (att.class === "image") {
      // Preview via data URL — the webview CSP is `img-src 'self' data:`,
      // so blob: object URLs never render. We already hold the base64.
      const mime = file.type || "image/png";
      att.thumbUrl = `data:${mime};base64,${b64}`;
    }
    state.attachments.push(att);
    renderChips();
    return att;
  } catch (e) {
    composerHint((e && e.message) || `Couldn't attach ${name}`);
    return null;
  }
}

// ── model picker ────────────────────────────────────────────────────

let modelCache = { at: 0, models: [] };
let modelFetch = null; // in-flight promise — double-open never double-fetches
const MODEL_CACHE_MS = 5 * 60 * 1000;

async function fetchModels(force = false) {
  const now = Date.now();
  if (!force && modelCache.models.length && now - modelCache.at < MODEL_CACHE_MS) {
    return modelCache.models;
  }
  if (!modelFetch) {
    modelFetch = (async () => {
      try {
        const raw = await call("list_models");
        const models = Array.isArray(raw) ? raw : JSON.parse(raw);
        modelCache = { at: Date.now(), models };
        return models;
      } finally {
        modelFetch = null;
      }
    })();
  }
  return modelFetch;
}

/** For tests: inject a catalog without the network. */
export function _setModelCacheForTest(models) {
  modelCache = { at: Date.now(), models };
}

function pickerEls() {
  return {
    pop: document.getElementById("modelPicker"),
    input: document.getElementById("modelPickerInput"),
    list: document.getElementById("modelPickerList"),
    status: document.getElementById("modelPickerStatus"),
  };
}

let pickerState = { open: false, models: [], filtered: [], active: 0, current: "" };

function renderPickerList() {
  const { list } = pickerEls();
  if (!list) return;
  const rows = pickerState.filtered.slice(0, 60);
  list.innerHTML = rows
    .map((m, i) => {
      const meta = [contextLabel(m), modelPriceLabel(m)].filter(Boolean).join(" · ");
      const badges = [
        m.native_tools ? `<span class="mp-badge">tools</span>` : "",
        m.vision ? `<span class="mp-badge mp-badge-vision">sees images</span>` : "",
      ].join("");
      return `<li class="mp-row${i === pickerState.active ? " active" : ""}${m.id === pickerState.current ? " current" : ""}" data-model="${esc(m.id)}" role="option" aria-selected="${i === pickerState.active}">
        <span class="mp-name">${esc(m.name || m.id)}</span>${badges}
        <span class="mp-meta">${esc(m.id)}${meta ? ` · ${meta}` : ""}</span>
      </li>`;
    })
    .join("");
  const { status } = pickerEls();
  if (status) {
    status.textContent = pickerState.filtered.length
      ? `${pickerState.filtered.length} models`
      : "No models match — try fewer letters";
  }
}

function applyFilter(q) {
  pickerState.filtered = filterModels(pickerState.models, q);
  pickerState.active = 0;
  renderPickerList();
}

async function selectModel(id) {
  if (!id) return;
  try {
    await call("set_model", { model: id });
    pickerState.current = id;
    const pill = document.getElementById("modelPillText");
    if (pill) pill.textContent = id;
    closePicker();
    composerHint(`Model set to ${id}`);
  } catch (e) {
    composerHint((e && e.message) || "Couldn't set the model");
  }
}

export async function openPicker(currentModel) {
  const { pop, input, status } = pickerEls();
  if (!pop) return;
  pickerState.open = true;
  pickerState.current = currentModel || pickerState.current;
  pop.hidden = false;
  if (input) {
    input.value = "";
    input.focus();
  }
  if (status) status.textContent = "Loading the live catalog…";
  try {
    pickerState.models = await fetchModels();
    applyFilter("");
  } catch (e) {
    if (status) status.textContent = (e && e.message) || "Couldn't load models";
    pickerState.models = [];
    pickerState.filtered = [];
    renderPickerList();
  }
}

export function closePicker() {
  const { pop } = pickerEls();
  if (pop) pop.hidden = true;
  pickerState.open = false;
  const ta = document.getElementById("chatInput");
  if (ta) ta.focus();
}

function movePickerActive(delta) {
  const n = Math.min(pickerState.filtered.length, 60);
  if (!n) return;
  pickerState.active = (pickerState.active + delta + n) % n;
  renderPickerList();
  const { list } = pickerEls();
  const el = list && list.querySelector(".mp-row.active");
  if (el && el.scrollIntoView) el.scrollIntoView({ block: "nearest" });
}

// ── init / wiring ───────────────────────────────────────────────────

export function initComposer({ getConversationId, currentModel } = {}) {
  if (typeof getConversationId === "function") {
    state.getConversationId = getConversationId;
  }
  pickerState.current = currentModel || "";
  const ta = document.getElementById("chatInput");
  const composer = document.getElementById("composer");
  const attachBtn = document.getElementById("attachBtn");
  const fileInput = document.getElementById("attachInput");
  const chipsRow = document.getElementById("composerAttachments");
  const pill = document.getElementById("modelPill");
  const { pop, input: pickerInput, list: pickerList } = pickerEls();

  // Paperclip → hidden file input.
  if (attachBtn && fileInput) {
    attachBtn.addEventListener("click", () => fileInput.click());
    fileInput.addEventListener("change", async () => {
      for (const f of Array.from(fileInput.files || [])) await stageFile(f);
      fileInput.value = "";
      if (ta) ta.focus();
    });
  }
  // Wand → a two-choice menu: quick tidy, or a full power-prompt rewrite.
  const refineBtn = document.getElementById("refineBtn");
  const refineMenu = document.getElementById("refineMenu");
  const closeRefineMenu = () => {
    if (refineMenu) refineMenu.hidden = true;
  };
  if (refineBtn && refineMenu) {
    refineBtn.addEventListener("click", () => {
      refineMenu.hidden = !refineMenu.hidden;
      if (!refineMenu.hidden) refineMenu.querySelector(".refine-opt")?.focus();
    });
    refineMenu.addEventListener("click", (e) => {
      const opt = e.target.closest(".refine-opt");
      if (!opt) return;
      closeRefineMenu();
      refineDraft(opt.dataset.mode || "tidy");
    });
    refineMenu.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        closeRefineMenu();
        refineBtn.focus();
      }
    });
    document.addEventListener("click", (e) => {
      if (refineMenu.hidden) return;
      if (refineMenu.contains(e.target) || refineBtn.contains(e.target)) return;
      closeRefineMenu();
    });
  } else if (refineBtn) {
    refineBtn.addEventListener("click", () => refineDraft("tidy"));
  }

  // Chip removal (delegated).
  if (chipsRow) {
    chipsRow.addEventListener("click", (e) => {
      const btn = e.target.closest(".attach-remove");
      if (!btn) return;
      const chip = btn.closest(".attach-chip");
      if (chip) removeAttachment(chip.dataset.att);
      if (ta) ta.focus();
    });
  }

  // Paste: screenshots become image attachments; very long text becomes a
  // text attachment; a bare URL gets an honest "I'll read it" hint.
  if (ta) {
    ta.addEventListener("paste", async (e) => {
      const cd = e.clipboardData;
      if (!cd) return;
      const items = Array.from(cd.items || []);
      const imgs = items.filter((it) => it.kind === "file" && /^image\//.test(it.type));
      if (imgs.length) {
        e.preventDefault();
        const stamp = new Date().toISOString().slice(11, 19).replace(/:/g, "");
        let staged = 0;
        for (const it of imgs) {
          const f = it.getAsFile();
          if (!f) continue;
          const ext = (it.type.split("/")[1] || "png").replace("jpeg", "jpg");
          screenshotSeq += 1;
          const ok = await stageFile(f, `screenshot-${stamp}-${screenshotSeq}.${ext}`);
          if (ok) staged += 1;
        }
        composerHint(
          staged > 1
            ? `${staged} screenshots attached — I'll look at them with your message`
            : "Screenshot attached — I'll look at it with your message",
        );
        return;
      }
      const text = cd.getData("text/plain");
      const kind = classifyPaste(text);
      if (kind === "attach") {
        e.preventDefault();
        const blob = new Blob([text], { type: "text/plain" });
        await stageFile(blob, "pasted-text.txt");
        composerHint(`Long paste (${formatSize(text.length)}) attached as text — the box stays readable`);
      } else if (kind === "url") {
        composerHint("I'll read that page as a source when you send");
      }
    });
  }

  // Drag styling + Rust-observed drops. The OS paths never reach JS; Rust
  // holds one-use grants and we claim opaque handles.
  if (composer) {
    for (const ev of ["dragenter", "dragover"]) {
      composer.addEventListener(ev, (e) => {
        e.preventDefault();
        composer.classList.add("drag-over");
      });
    }
    for (const ev of ["dragleave", "drop"]) {
      composer.addEventListener(ev, (e) => {
        e.preventDefault();
        composer.classList.remove("drag-over");
      });
    }
  }
  on("file_drop_ready", async () => {
    const scope = attachScope();
    // Claim every pending grant (multi-file drops).
    for (let i = 0; i < 8; i++) {
      let res;
      try {
        res = await call("claim_dropped_file", { owner: scope });
      } catch {
        break;
      }
      if (!res || !res.artifact_id) break;
      state.attachments.push({
        artifact_id: res.artifact_id,
        scope,
        name: res.label || "file",
        class: res.class || "text",
        size: 0,
      });
    }
    renderChips();
    if (ta) ta.focus();
  });

  // Model pill → picker.
  if (pill) {
    pill.addEventListener("click", () => {
      if (pickerState.open) closePicker();
      else openPicker();
    });
  }
  if (pickerInput) {
    pickerInput.addEventListener("input", () => applyFilter(pickerInput.value));
    pickerInput.addEventListener("keydown", (e) => {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        movePickerActive(1);
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        movePickerActive(-1);
      } else if (e.key === "Enter") {
        e.preventDefault();
        const m = pickerState.filtered[pickerState.active];
        if (m) selectModel(m.id);
      } else if (e.key === "Escape") {
        e.preventDefault();
        closePicker();
      }
    });
  }
  if (pickerList) {
    pickerList.addEventListener("click", (e) => {
      const row = e.target.closest(".mp-row");
      if (row) selectModel(row.dataset.model);
    });
  }
  if (pop) {
    // Click-away closes.
    document.addEventListener("click", (e) => {
      if (!pickerState.open) return;
      if (pop.contains(e.target) || (pill && pill.contains(e.target))) return;
      closePicker();
    });
  }
}
