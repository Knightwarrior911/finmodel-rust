// analyst.mjs — Analyst tools modal (Phase 6.5): EV bridge, IFRS bridge, and
// tie-out. Each form invokes the matching desktop command (fm-value / fm-ifrs /
// fm-tieout) and renders the result. These are explicit, selected actions — one
// per submit — never a flat auto-tool list handed to a model.

import { $, call, escapeHtml, activateDialog } from "./core.mjs";

let deactivate = null;

function status(msg, kind = "info") {
  const el = $("analystStatus");
  if (!el) return;
  if (!msg) {
    el.hidden = true;
    el.textContent = "";
    return;
  }
  el.hidden = false;
  el.className = `status ${kind}`;
  el.textContent = msg;
}

function m$(v) {
  return v == null ? "—" : `${Number(v).toLocaleString()}M`;
}

// Read a form's numeric/text fields into a plain object; blank numbers are
// omitted (so the EV checklist applies only entered items).
function readForm(form, { numeric }) {
  const out = {};
  for (const el of form.elements) {
    if (!el.name) continue;
    if (el.type === "number") {
      const raw = el.value.trim();
      if (raw === "") continue;
      out[el.name] = Number(raw);
    } else {
      out[el.name] = el.value;
    }
  }
  // Numeric fields the caller marks required but that stayed blank → surface.
  for (const key of numeric || []) {
    if (out[key] == null) out[key] = 0;
  }
  return out;
}

function renderEv(b) {
  const line = (li) =>
    `<div class="analyst-row"><span>${escapeHtml(li.item)}</span><span class="num">${m$(
      li.amount
    )}</span><span class="analyst-src">${escapeHtml(li.source)}</span></div>`;
  const adds = (b.additions || []).map(line).join("");
  const subs = (b.subtractions || []).map(line).join("");
  return `
    <div class="analyst-row analyst-total"><span>Equity value (market cap)</span><span class="num">${m$(
      b.market_cap
    )}</span><span></span></div>
    ${adds ? `<div class="analyst-sec">(+) additions</div>${adds}` : ""}
    ${subs ? `<div class="analyst-sec">(−) subtractions</div>${subs}` : ""}
    <div class="analyst-row analyst-total"><span>= Enterprise value</span><span class="num">${m$(
      b.total_ev
    )}</span><span></span></div>`;
}

function renderIfrs(o) {
  // The output carries adjusted values + deltas + both margins (not the raw
  // reported EBIT/EBITDA), so show Adjusted / Δ and Reported% / Adjusted%.
  const val = (label, adj, delta) =>
    `<div class="analyst-row"><span>${escapeHtml(label)}</span><span class="num">${m$(
      adj
    )}</span><span class="num">${m$(delta)}</span></div>`;
  const marg = (label, rep, adj) =>
    `<div class="analyst-row"><span>${escapeHtml(label)}</span><span class="num">${rep.toFixed(
      1
    )}%</span><span class="num">${adj.toFixed(1)}%</span></div>`;
  return `
    <div class="analyst-row analyst-sec"><span>Direction</span><span>${escapeHtml(
      String(o.direction)
    )}</span><span></span></div>
    <div class="analyst-row"><span></span><span>Adjusted</span><span>Δ</span></div>
    ${val("EBIT", o.adjusted_ebit, o.ebit_delta)}
    ${val("EBITDA", o.adjusted_ebitda, o.ebitda_delta)}
    ${val("EBITA", o.adjusted_ebita, o.ebita_delta)}
    <div class="analyst-row analyst-sec"><span>Margins</span><span>Reported</span><span>Adjusted</span></div>
    ${marg("EBIT margin", o.reported_ebit_margin, o.adjusted_ebit_margin)}
    ${marg("EBITDA margin", o.reported_ebitda_margin, o.adjusted_ebitda_margin)}
    ${marg("EBITA margin", o.reported_ebita_margin, o.adjusted_ebita_margin)}`;
}

function renderTieout(s) {
  const mism = (s.mismatches || [])
    .map((m) => `<div class="analyst-row"><span>${escapeHtml(String(m.key || m))}</span></div>`)
    .join("");
  return `
    <div class="analyst-row analyst-total"><span>Matched</span><span class="num">${s.matched} / ${s.trusted}</span><span class="num">${s.percentage.toFixed(
    1
  )}%</span></div>
    ${mism ? `<div class="analyst-sec">Mismatches</div>${mism}` : `<p class="field-hint">No mismatches.</p>`}`;
}

function showTab(tab) {
  for (const btn of document.querySelectorAll(".analyst-tab")) {
    btn.setAttribute("aria-selected", btn.dataset.tab === tab ? "true" : "false");
  }
  for (const panel of document.querySelectorAll(".analyst-panel")) {
    panel.hidden = panel.dataset.panel !== tab;
  }
  status("");
  $("analystResult").innerHTML = "";
}

export function openAnalyst() {
  const modal = $("analystModal");
  if (!modal) return;
  modal.hidden = false;
  showTab("ev");
  deactivate = activateDialog(modal.querySelector(".modal-card"), {
    initialFocus: ".analyst-tab",
    onEscape: closeAnalyst,
  });
}

function closeAnalyst() {
  const modal = $("analystModal");
  if (modal) modal.hidden = true;
  if (deactivate) {
    deactivate();
    deactivate = null;
  }
}

async function submit(form, command, extra, render) {
  status("Computing…");
  $("analystResult").innerHTML = "";
  try {
    const payload = { ...extra(form) };
    const res = await call(command, payload);
    status("");
    $("analystResult").innerHTML = render(res);
  } catch (err) {
    status(`Failed: ${err && err.message ? err.message : err}`, "error");
  }
}

export function initAnalyst() {
  const modal = $("analystModal");
  if (!modal) return;
  $("analystClose").addEventListener("click", closeAnalyst);
  modal.querySelector(".modal-backdrop").addEventListener("click", closeAnalyst);
  for (const btn of modal.querySelectorAll(".analyst-tab")) {
    btn.addEventListener("click", () => showTab(btn.dataset.tab));
  }
  $("evForm").addEventListener("submit", (e) => {
    e.preventDefault();
    const f = e.target;
    const input = readForm(f, {});
    // Equity value is required: the core falls back to 0, which would present
    // debt−cash as "Enterprise value" with no equity base (financially invalid).
    const hasEquity =
      input.market_cap > 0 ||
      (input.share_price > 0 && input.shares_outstanding > 0);
    if (!hasEquity) {
      status("Enter market cap, or both share price and shares.", "error");
      $("analystResult").innerHTML = "";
      return;
    }
    submit(f, "ev_bridge", () => ({ input }), renderEv);
  });
  $("ifrsForm").addEventListener("submit", (e) => {
    e.preventDefault();
    submit(
      e.target,
      "ifrs_bridge",
      (f) => {
        const all = readForm(f, {
          numeric: [
            "rou_depreciation",
            "lease_interest",
            "short_term_rent",
            "reported_ebit",
            "reported_ebitda",
            "reported_ebita",
            "standard_depreciation",
            "standard_amortization",
          ],
        });
        const revenue = all.revenue || 0;
        delete all.revenue;
        return { input: all, revenue };
      },
      renderIfrs
    );
  });
  $("tieoutForm").addEventListener("submit", (e) => {
    e.preventDefault();
    submit(
      e.target,
      "tie_out",
      (f) => ({
        ground_truth_json: f.elements.ground_truth_json.value,
        model_json: f.elements.model_json.value,
      }),
      renderTieout
    );
  });
}
