// finmodel frontend — talks to the Rust backend via Tauri invoke.
const TAURI = window.__TAURI__;
const invokeRaw = TAURI ? TAURI.core.invoke : null;

// Every command returns a JSON *string* (or throws an AppError object).
async function call(name, payload = {}) {
  if (!invokeRaw) throw new Error("Not running inside the app window.");
  const res = await invokeRaw(name, payload);
  return typeof res === "string" ? JSON.parse(res) : res;
}

const $ = (id) => document.getElementById(id);

// Escape untrusted values before any innerHTML interpolation. Update metadata
// (version/notes) comes from a remote latest.json whose *string fields are not
// signature-verified* (only the downloaded artifact is), so treat them as hostile.
function escapeHtml(s) {
  return String(s == null ? "" : s).replace(
    /[&<>"']/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c])
  );
}

// Minimal, SANITIZED markdown → HTML for the in-app reader (Phase 8.4). Escape
// EVERYTHING first, then re-inject only a whitelist: headings, paragraphs,
// lists, and http(s) links. No raw HTML, no <script>/on* ever survives.
function renderMarkdown(md) {
  const esc = escapeHtml(String(md == null ? "" : md));
  const lines = esc.split(/\r?\n/);
  const out = [];
  let inList = false;
  const closeList = () => { if (inList) { out.push("</ul>"); inList = false; } };
  // Inline: bold, code, and safe links [text](http…) — operate on already-escaped text.
  const inline = (t) => t
    .replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g,
      (_m, txt, url) => `<a href="#" class="md-link" data-url="${url}">${txt}</a>`)
    .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
    .replace(/`([^`]+)`/g, "<code>$1</code>");
  for (const raw of lines) {
    const line = raw.trim();
    const h = line.match(/^(#{1,4})\s+(.*)$/);
    const li = line.match(/^[-*]\s+(.*)$/);
    if (h) { closeList(); const n = h[1].length; out.push(`<h${n}>${inline(h[2])}</h${n}>`); }
    else if (li) { if (!inList) { out.push("<ul>"); inList = true; } out.push(`<li>${inline(li[1])}</li>`); }
    else if (line === "") { closeList(); }
    else { closeList(); out.push(`<p>${inline(line)}</p>`); }
  }
  closeList();
  return out.join("");
}
let lastModel = null;
let activeStmt = "income_statement";
// Single-flight guard: only one build/benchmark runs at a time. Overlapping
// invokes race the shared process-global settings and the results panes.
let busy = false;

function setStatus(msg, kind = "info") {
  const el = $("status");
  el.hidden = false;
  el.textContent = msg;
  el.className = `status ${kind}`;
}

function clearStatus() {
  $("status").hidden = true;
}

function fmtNum(v) {
  if (v === null || v === undefined) return "—";
  const n = Number(v);
  if (!isFinite(n)) return "—";
  return n.toLocaleString(undefined, { maximumFractionDigits: 1 });
}

function prettyKey(k) {
  return k.replace(/_/g, " ").replace(/\b\w/g, (c) => c.toUpperCase());
}

// ── Advanced build options + assumptions grid ───────────────────────
let lastSession = null;      // prepare_model session id
let lastProjPeriods = [];    // projection year labels for the grid
let buildOutPath = null;     // Save-As chosen path (build)
let benchOutPath = null;     // Save-As chosen path (benchmark)

// A percentage field (human "4.5") → decimal (0.045). Blank → null (use default).
function pctToDec(id) {
  const v = ($(id).value || "").trim();
  if (v === "") return null;
  const n = Number(v);
  return isFinite(n) ? n / 100 : null;
}
// A plain numeric field → number, blank → null.
function numOr(id) {
  const v = ($(id).value || "").trim();
  if (v === "") return null;
  const n = Number(v);
  return isFinite(n) ? n : null;
}

// Read the Advanced options panel into a BuildOptions object (omitted fields
// fall back to engine defaults via serde). Percentages convert human→decimal.
function collectBuildOptions() {
  const o = { sector: $("optSector").value, fiscal_year_end: $("optFye").value };
  const years = numOr("optYears"); if (years != null) o.proj_years = Math.round(years);
  const rf = pctToDec("optRiskFree"); if (rf != null) o.risk_free_rate = rf;
  const erp = pctToDec("optErp"); if (erp != null) o.equity_risk_premium = erp;
  const de = numOr("optTargetDe"); if (de != null) o.target_de_ratio = de;
  const kd = pctToDec("optCostDebt"); if (kd != null) o.cost_of_debt_pretax = kd;
  const beta = numOr("optBeta"); if (beta != null) o.beta = beta;
  const tax = pctToDec("optTaxRate"); if (tax != null) o.tax_rate_override = tax;
  const tg = pctToDec("optTerminalGrowth"); if (tg != null) o.terminal_growth = tg;
  const exit = numOr("optExitMult"); if (exit != null) o.exit_ebitda_multiple = exit;
  const tv = document.querySelector('input[name="tvMethod"]:checked');
  if (tv) o.tv_method = Number(tv.value);
  const sp = numOr("optSharePrice"); if (sp != null) o.share_price = sp;
  if (buildOutPath) o.out_path = buildOutPath;
  return o;
}

// Render the editable per-year assumptions grid from a prepare_model response.
function renderAssumptions(prep) {
  lastSession = prep.session_id;
  lastProjPeriods = prep.proj_periods || [];
  const thead = $("assumptionsTable").querySelector("thead");
  const tbody = $("assumptionsTable").querySelector("tbody");
  const cols = ["Driver", ...lastProjPeriods];
  thead.innerHTML = "<tr>" + cols.map((c) => `<th>${escapeHtml(c)}</th>`).join("") + "</tr>";
  const keys = Object.keys(prep.labels || {});
  tbody.innerHTML = keys.map((key) => {
    const label = prep.labels[key] || key;
    const vals = prep.drivers[key] || [];
    const cells = lastProjPeriods.map((_, i) => {
      const v = vals[i];
      const s = v == null ? "" : String(Math.round(v * 1e6) / 1e6);
      return `<td><input type="number" step="0.0001" data-orig="${escapeHtml(s)}" value="${escapeHtml(s)}"></td>`;
    }).join("");
    return `<tr data-key="${escapeHtml(key)}"><td class="lbl">${escapeHtml(label)}</td>${cells}</tr>`;
  }).join("");
  tbody.querySelectorAll("input").forEach((inp) => {
    inp.addEventListener("input", () => {
      inp.classList.toggle("edited", inp.value.trim() !== (inp.dataset.orig || ""));
    });
  });
  $("assumptionsPanel").hidden = false;
  $("assumptionsPanel").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

// Only edited cells become overrides; blank/unchanged → null (keep derived).
// Values are engine-native fractions (0.05 = 5%).
function collectOverrides() {
  const overrides = [];
  document.querySelectorAll("#assumptionsTable tbody tr").forEach((tr) => {
    const values = Array.from(tr.querySelectorAll("input")).map((inp) => {
      if (!inp.classList.contains("edited") || inp.value.trim() === "") return null;
      const n = Number(inp.value.trim());
      return isFinite(n) ? n : null;
    });
    if (values.some((v) => v != null)) overrides.push({ key: tr.dataset.key, values });
  });
  return overrides;
}

// ── Mode banner (offline demo vs live) ──────────────────────────────
async function initMode() {
  let hasKey = false;
  let model = "";
  try {
    const s = await call("load_settings");
    hasKey = !!s.has_key;
    model = s.model || "";
    if (s.version) $("appVersion").textContent = `finmodel v${s.version}`;
  } catch (_) {
    // Not in the app window (browser preview) or first launch → treat as demo.
  }
  const pill = $("modePill");
  const pillText = $("modePillText");
  const banner = $("modeBanner");
  const bannerText = $("modeBannerText");
  if (hasKey) {
    pill.classList.add("live");
    pillText.textContent = "Live";
    pill.title = model ? `Live extraction · ${model}` : "Live extraction";
    banner.className = "banner ok";
    bannerText.innerHTML =
      `<strong>Live mode.</strong> Build a full model for any US ticker (SEC EDGAR) ` +
      `or international company (annual-report PDF), and benchmark any US peer set.` +
      (model ? ` Model: <code>${escapeHtml(model)}</code>.` : "");
  } else {
    pill.classList.remove("live");
    pillText.textContent = "Demo mode";
    pill.title = "No API key — offline demo";
    banner.className = "banner info";
    bannerText.innerHTML =
      `<strong>No API key set.</strong> <strong>Benchmarking</strong> works right now ` +
      `for any US tickers. <strong>Building a full model</strong> works for the 5 demo ` +
      `companies below — add an OpenRouter key to build any company worldwide.`;
  }
}

// ── Live input parsing / validation ─────────────────────────────────
function normTicker(s) {
  return (s || "").trim().toUpperCase();
}

function parsePeers(s) {
  return (s || "")
    .split(",")
    .map((t) => t.trim().toUpperCase())
    .filter((t) => t.length > 0);
}

function updateTickerUI() {
  const raw = $("ticker").value;
  const t = normTicker(raw);
  const echo = $("tickerEcho");
  $("buildBtn").disabled = t.length === 0;
  if (t && t !== raw.trim()) {
    echo.hidden = false;
    echo.textContent = `→ will read as ${t}`;
  } else {
    echo.hidden = true;
  }
}

function updatePeersUI() {
  const list = parsePeers($("peers").value);
  const echo = $("peersEcho");
  $("benchBtn").disabled = list.length < 2; // need ≥2 tickers to compare
  if (list.length === 0) {
    echo.hidden = true;
  } else if (list.length === 1) {
    echo.hidden = false;
    echo.textContent = "1 company — add at least one more to compare";
  } else {
    echo.hidden = false;
    echo.textContent = `${list.length} companies: ${list.join(", ")}`;
  }
}

function renderTable() {
  if (!lastModel) return;
  const hist = lastModel.hist[activeStmt] || {};
  const proj = lastModel.proj[activeStmt] || {};
  const histCols = lastModel.hist_periods || [];
  const projCols = lastModel.proj_periods || [];

  const thead = $("modelTable").querySelector("thead");
  const tbody = $("modelTable").querySelector("tbody");
  const head = ["Item", ...histCols, ...projCols]
    .map((h, i) => {
      const isLbl = i === 0;
      const isProj = i > histCols.length;
      return `<th class="${isLbl ? "lbl" : "num"}${isProj ? " proj" : ""}">${escapeHtml(h)}</th>`;
    })
    .join("");
  thead.innerHTML = `<tr>${head}</tr>`;

  // Canonical row order from the backend first; unknown keys stable-appended
  // alphabetically. Never .sort() the whole set (that put COGS above Revenue).
  const present = new Set([...Object.keys(hist), ...Object.keys(proj)]);
  const canonical = (lastModel.order && lastModel.order[activeStmt]) || [];
  const ordered = canonical.filter((k) => present.has(k));
  const canonSet = new Set(canonical);
  const extras = [...present].filter((k) => !canonSet.has(k)).sort();
  const keys = [...ordered, ...extras];

  tbody.innerHTML = keys
    .map((k) => {
      const hv = hist[k] || [];
      const pv = proj[k] || [];
      const cells = [];
      for (let i = 0; i < histCols.length; i++) {
        cells.push(`<td class="num">${fmtNum(hv[i])}</td>`);
      }
      for (let i = 0; i < projCols.length; i++) {
        cells.push(`<td class="num proj">${fmtNum(pv[i])}</td>`);
      }
      return `<tr><td class="lbl">${escapeHtml(prettyKey(k))}</td>${cells.join("")}</tr>`;
    })
    .join("");
}

// Amber, dismissible data-quality notes (WACC clamp, Gordon TV, invariants).
function renderWarnings(warnings) {
  const el = $("modelWarnings");
  if (!el) return;
  const list = Array.isArray(warnings) ? warnings : [];
  if (list.length === 0) {
    el.hidden = true;
    el.innerHTML = "";
    return;
  }
  const items = list.map((w) => `<li>${escapeHtml(w)}</li>`).join("");
  el.innerHTML =
    `<button type="button" class="warn-dismiss" aria-label="Dismiss">×</button>` +
    `<strong>Data notes (${list.length})</strong><ul class="warn-list">${items}</ul>`;
  el.hidden = false;
  const btn = el.querySelector(".warn-dismiss");
  if (btn) btn.addEventListener("click", () => { el.hidden = true; });
}

async function build(ticker) {
  if (busy) return;
  clearStatus();
  const options = collectBuildOptions();
  const skip = $("skipReview").checked || localStorage.getItem("skipReview") === "1";
  busy = true;
  document.body.classList.add("busy");
  $("buildBtn").disabled = true;
  const label = $("buildBtn").querySelector(".btn-label");
  if (label) label.textContent = skip ? "Building…" : "Preparing…";
  try {
    if (skip) {
      setStatus(`Building model for ${ticker}… reading the filing, projecting, writing Excel.`, "info");
      const model = await call("build_model", { ticker, options });
      renderModelResult(model);
    } else {
      setStatus(`Preparing assumptions for ${ticker}…`, "info");
      const prep = await call("prepare_model", { ticker, options });
      renderAssumptions(prep);
      setStatus("Review the assumptions below, then Build Excel.", "info");
    }
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    setStatus(`Could not build ${ticker}: ${msg}`, "error");
    $("results").hidden = true;
  } finally {
    busy = false;
    document.body.classList.remove("busy");
    $("buildBtn").disabled = false;
    if (label) label.textContent = "Build model";
    updateTickerUI();
  }
}

// Render a completed model summary (shared by one-click + finalize paths).
function renderModelResult(model) {
  lastModel = model;
  $("assumptionsPanel").hidden = true;
  $("results").hidden = false;
  $("resTitle").textContent = `${model.ticker}  ·  ${model.currency}`;
  $("resMeta").textContent =
    `Source: ${model.source}  ·  Historical ${model.hist_periods.join(", ")}  ·  Projected ${model.proj_periods.join(", ")}`;
  renderTable();
  renderWarnings(model.warnings);
  renderValuation(model.valuation);
  clearStatus();
  renderRecent();
  renderNews(model.ticker);
  $("results").scrollIntoView({ behavior: "smooth", block: "nearest" });
}

// Step 2: apply the grid overrides and build the workbook.
async function finalizeModel() {
  if (busy || !lastSession) return;
  const options = collectBuildOptions();
  options.assumption_overrides = collectOverrides();
  busy = true;
  document.body.classList.add("busy");
  $("finalizeBtn").disabled = true;
  try {
    const model = await call("finalize_model", { session_id: lastSession, options });
    renderModelResult(model);
  } catch (e) {
    const el = $("assumptionsStatus");
    el.hidden = false;
    el.textContent = `Build failed: ${e.message || e}`;
    el.className = "status error";
  } finally {
    busy = false;
    document.body.classList.remove("busy");
    $("finalizeBtn").disabled = false;
  }
}

// Compact valuation strip (4.3): implied price, upside, WACC, EV.
function renderValuation(v) {
  const el = $("valuationStrip");
  if (!el) return;
  if (!v || !v.has_dcf) { el.hidden = true; return; }
  const price = (x) => (x == null ? "—" : Number(x).toFixed(2));
  const pct = (x) => (x == null ? "—" : (x * 100).toFixed(1) + "%");
  const money = (x) => (x == null ? "—" : Math.round(x / 1e6).toLocaleString() + "M");
  if (v.current_price && v.price_per_share != null) {
    const up = v.upside_pct;
    const cls = up == null ? "" : up >= 0 ? "up" : "down";
    el.innerHTML =
      `<span class="val-item"><span class="val-k">Implied</span><span class="val-v">${escapeHtml(price(v.price_per_share))}</span></span>` +
      `<span class="val-item"><span class="val-k">Current</span><span class="val-v">${escapeHtml(price(v.current_price))}</span></span>` +
      `<span class="val-item ${cls}"><span class="val-k">Upside</span><span class="val-v">${escapeHtml(pct(up))}</span></span>` +
      `<span class="val-item"><span class="val-k">WACC</span><span class="val-v">${escapeHtml(pct(v.wacc))}</span></span>` +
      `<span class="val-item"><span class="val-k">EV</span><span class="val-v">${escapeHtml(money(v.ev))}</span></span>` +
      `<span class="val-method">${escapeHtml(v.method || "")}</span>`;
  } else {
    el.innerHTML = `<span class="val-note">Add a share price in Advanced options for DCF upside.</span>`;
  }
  el.hidden = false;
}

// Latest headlines strip (Phase 5). Fire-and-forget; failure hides the strip.
async function renderNews(query) {
  const el = $("newsStrip");
  if (!el) return;
  el.hidden = true;
  el.innerHTML = "";
  let list = [];
  try { list = await call("get_news", { query, limit: 5 }); } catch (_) { return; }
  if (!Array.isArray(list) || !list.length) return;
  el.innerHTML =
    `<div class="news-head">Latest headlines</div>` +
    list.map((h) =>
      `<a class="news-item" data-url="${escapeHtml(h.url)}" href="#">` +
      `${escapeHtml(h.title)} <span class="news-src">${escapeHtml(h.source || "")}</span></a>`
    ).join("");
  el.querySelectorAll(".news-item").forEach((a) => {
    a.addEventListener("click", (e) => {
      e.preventDefault();
      call("open_url", { url: a.dataset.url }).catch(() => {});
    });
  });
  el.hidden = false;
}

// ---- events ----
$("ticker").addEventListener("input", updateTickerUI);
$("buildBtn").addEventListener("click", () => {
  const t = normTicker($("ticker").value);
  if (t) build(t);
});
$("ticker").addEventListener("keydown", (e) => {
  const t = normTicker($("ticker").value);
  if (e.key === "Enter" && t) build(t);
});
$("demoChips").addEventListener("click", (e) => {
  const b = e.target.closest(".chip");
  if (b) {
    $("ticker").value = b.dataset.t;
    updateTickerUI();
    build(b.dataset.t);
  }
});
$("finalizeBtn").addEventListener("click", finalizeModel);
$("resetAssumptions").addEventListener("click", () => {
  document.querySelectorAll("#assumptionsTable tbody input").forEach((inp) => {
    inp.value = inp.dataset.orig || "";
    inp.classList.remove("edited");
  });
});
$("skipReview").addEventListener("change", (e) => {
  localStorage.setItem("skipReview", e.target.checked ? "1" : "0");
});
if (localStorage.getItem("skipReview") === "1") $("skipReview").checked = true;

// Save-As via the dialog plugin. Returns the chosen path (or null).
async function chooseSavePath(defaultName) {
  const dlg = TAURI && TAURI.dialog;
  if (!dlg) return null;
  try {
    return await dlg.save({
      defaultPath: defaultName,
      filters: [{ name: "Excel", extensions: ["xlsx"] }],
    });
  } catch (_) {
    return null;
  }
}
$("buildChangeDir").addEventListener("click", async () => {
  const t = (normTicker($("ticker").value) || "model").replace(/[./]/g, "_");
  const p = await chooseSavePath(`${t}_model.xlsx`);
  if (p) { buildOutPath = p; $("buildOutDir").textContent = p; }
});
$("benchChangeDir").addEventListener("click", async () => {
  const p = await chooseSavePath("benchmark.xlsx");
  if (p) { benchOutPath = p; $("benchOutDir").textContent = p; }
});
$("tabs").addEventListener("click", (e) => {
  const b = e.target.closest(".tab");
  if (!b) return;
  document.querySelectorAll(".tab").forEach((t) => {
    t.classList.remove("active");
    t.setAttribute("aria-selected", "false");
  });
  b.classList.add("active");
  b.setAttribute("aria-selected", "true");
  activeStmt = b.dataset.s;
  renderTable();
});
$("openXlsxBtn").addEventListener("click", async () => {
  if (lastModel && lastModel.xlsx_path) {
    try {
      await call("open_path", { path: lastModel.xlsx_path });
    } catch (e) {
      setStatus(`Open failed: ${e.message || e}`, "error");
    }
  }
});

// ---- peer benchmark ----
let lastBench = null;

function setBenchStatus(msg, kind = "info") {
  const el = $("benchStatus");
  el.hidden = false;
  el.textContent = msg;
  el.className = `status ${kind}`;
}

function fmtPct(v) { return v == null ? "—" : (v * 100).toFixed(1) + "%"; }
function fmtMult(v) { return v == null ? "—" : v.toFixed(1) + "x"; }
function fmtM(v) { return v == null ? "—" : Math.round(v).toLocaleString(); }

const BENCH_COLS = [
  ["ticker", "Ticker", (r) => r.ticker || "—"],
  ["sector", "Sector", (r) => r.sector || "—"],
  ["fiscal_year", "FY", (r) => r.fiscal_year || "—"],
  ["revenue_m", "Revenue ($M)", (r) => fmtM(r.revenue_m)],
  ["ebitda_m", "EBITDA ($M)", (r) => fmtM(r.ebitda_m)],
  ["rev_growth", "Rev Growth", (r) => fmtPct(r.rev_growth)],
  ["ebitda_margin", "EBITDA %", (r) => fmtPct(r.ebitda_margin)],
  ["net_margin", "Net %", (r) => fmtPct(r.net_margin)],
  ["roe", "ROE", (r) => fmtPct(r.roe)],
  ["net_debt_to_ebitda", "ND/EBITDA", (r) => fmtMult(r.net_debt_to_ebitda)],
];

function renderBench() {
  if (!lastBench) return;
  const thead = $("benchTable").querySelector("thead");
  const tbody = $("benchTable").querySelector("tbody");
  thead.innerHTML = "<tr>" + BENCH_COLS.map((c) => `<th>${escapeHtml(c[1])}</th>`).join("") + "</tr>";
  tbody.innerHTML = lastBench.rows
    .map((r) => "<tr>" + BENCH_COLS.map((c) => `<td>${escapeHtml(c[2](r))}</td>`).join("") + "</tr>")
    .join("");
}

function renderBenchWarnings(warnings) {
  const el = $("benchWarnings");
  if (!el) return;
  const list = Array.isArray(warnings) ? warnings : [];
  if (!list.length) { el.hidden = true; el.innerHTML = ""; return; }
  el.innerHTML =
    `<strong>Data notes (${list.length})</strong><ul class="warn-list">` +
    list.map((w) => `<li>${escapeHtml(w)}</li>`).join("") +
    `</ul>`;
  el.hidden = false;
}

async function benchmark(tickers) {
  if (busy) return;
  busy = true;
  document.body.classList.add("busy");
  $("benchBtn").disabled = true;
  const label = $("benchBtn").querySelector(".btn-label-b");
  if (label) label.textContent = "Benchmarking…";
  const list = parsePeers(tickers);
  setBenchStatus(`Fetching SEC filings for ${list.length} companies…`, "info");
  try {
    const opts = {
      period: $("benchPeriod").value,
      multiples: $("benchMultiples").checked,
      usd: $("benchUsd").checked,
      title: ($("benchTitle").value || "").trim() || null,
      out_path: benchOutPath,
    };
    const res = await call("benchmark_peers", { tickers, opts });
    lastBench = res;
    $("benchResults").hidden = false;
    $("benchTitle").textContent = `Peer Benchmark  ·  ${res.count} of ${res.requested}`;
    const fails = (res.failed || []).map((f) => `${f.ticker} (${f.why})`).join("; ");
    $("benchMeta").textContent = fails
      ? `SEC EDGAR XBRL · skipped: ${fails}`
      : `SEC EDGAR XBRL · ${res.count} peers · every number cites an exact filing fact`;
    renderBench();
    renderBenchWarnings(res.data_warnings);
    renderRecent();
    $("benchStatus").hidden = true;
    $("benchResults").scrollIntoView({ behavior: "smooth", block: "nearest" });
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    setBenchStatus(`Could not benchmark: ${msg}`, "error");
    $("benchResults").hidden = true;
  } finally {
    busy = false;
    document.body.classList.remove("busy");
    $("benchBtn").disabled = false;
    if (label) label.textContent = "Benchmark";
    updatePeersUI();
  }
}

$("peers").addEventListener("input", updatePeersUI);
$("benchBtn").addEventListener("click", () => {
  const t = $("peers").value.trim();
  if (t) benchmark(t);
});
$("peers").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && $("peers").value.trim()) benchmark($("peers").value.trim());
});
$("peerChips").addEventListener("click", (e) => {
  const b = e.target.closest(".chip");
  if (b) {
    $("peers").value = b.dataset.t;
    updatePeersUI();
    benchmark(b.dataset.t);
  }
});
$("openBenchBtn").addEventListener("click", async () => {
  if (lastBench && lastBench.xlsx_path) {
    try { await call("open_path", { path: lastBench.xlsx_path }); }
    catch (e) { setBenchStatus(`Open failed: ${e.message || e}`, "error"); }
  }
});
$("openBenchCsvBtn").addEventListener("click", async () => {
  if (lastBench && lastBench.csv_path) {
    try { await call("open_path", { path: lastBench.csv_path }); }
    catch (e) { setBenchStatus(`Open failed: ${e.message || e}`, "error"); }
  }
});

// ---- settings ----
const modal = $("settingsModal");

// Settings errors must show INSIDE the open modal — #status lives in the (often
// hidden) build card, so routing there makes save/refresh failures invisible.
function setSettingsStatus(msg, kind = "info") {
  const el = $("settingsStatus");
  if (!el) return;
  el.hidden = false;
  el.textContent = msg;
  el.className = `status ${kind}`;
}
function clearSettingsStatus() {
  const el = $("settingsStatus");
  if (el) el.hidden = true;
}

async function openSettings() {
  clearSettingsStatus();
  try {
    const s = await call("load_settings");
    $("keyStatus").textContent = s.has_key
      ? "A key is saved. Leave blank to keep it."
      : "No key set — offline demo tickers only.";
    const sel = $("modelSelect");
    if (s.model) sel.innerHTML = `<option value="${escapeHtml(s.model)}">${escapeHtml(s.model)}</option>`;
    $("edgarContact").value = s.edgar_contact || "";
    $("outDir").value = s.out_dir || "";
    $("mcpCommand").value = s.mcp_command || "";
    if (s.out_dir) {
      $("buildOutDir").textContent = s.out_dir;
      $("benchOutDir").textContent = s.out_dir;
    }
  } catch (_) {
    /* offline / first launch */
  }
  modal.hidden = false;
  $("apiKey").focus();
}

function closeSettings() {
  clearSettingsStatus();
  modal.hidden = true;
}

$("settingsBtn").addEventListener("click", openSettings);
$("bannerSettingsBtn").addEventListener("click", openSettings);
$("settingsClose").addEventListener("click", closeSettings);
modal.addEventListener("click", (e) => {
  if (e.target && e.target.dataset && e.target.dataset.close) closeSettings();
});
document.addEventListener("keydown", (e) => {
  if (e.key === "Escape" && !modal.hidden) closeSettings();
});

$("saveSettings").addEventListener("click", async () => {
  const api_key = $("apiKey").value;
  const model = $("modelSelect").value || "";
  const mcp_args = ($("mcpCommand").value || "").trim().split(/\s+/).slice(1);
  try {
    await call("save_settings", {
      api_key,
      model,
      edgar_contact: $("edgarContact").value,
      out_dir: $("outDir").value,
      mcp_command: ($("mcpCommand").value || "").trim().split(/\s+/)[0] || "",
      mcp_args,
    });
    $("apiKey").value = "";
    closeSettings();
    await initMode();
  } catch (e) {
    setSettingsStatus(`Save failed: ${e.message || e}`, "error");
  }
});

$("removeKey").addEventListener("click", async () => {
  try {
    await call("clear_api_key");
    $("apiKey").value = "";
    $("keyStatus").textContent = "No key set — offline demo tickers only.";
    setSettingsStatus("Key removed — offline demo mode.", "info");
    await initMode();
  } catch (e) {
    setSettingsStatus(`Remove failed: ${e.message || e}`, "error");
  }
});

$("chooseOutDir").addEventListener("click", async () => {
  const dlg = TAURI && TAURI.dialog;
  if (!dlg) return;
  try {
    const dir = await dlg.open({ directory: true });
    if (dir) $("outDir").value = dir;
  } catch (_) {
    /* cancelled */
  }
});

$("testMcp").addEventListener("click", async () => {
  const el = $("mcpStatus");
  const cmd = ($("mcpCommand").value || "").trim();
  if (!cmd) { el.textContent = "Enter the Roam MCP command first."; return; }
  el.textContent = "Testing…";
  try {
    const parts = cmd.split(/\s+/);
    const res = await call("test_mcp", { command: parts[0], args: parts.slice(1) });
    el.textContent = `Connected — ${res.tool_count} tools available.`;
  } catch (e) {
    el.textContent = `Connection failed: ${e.message || e}`;
  }
});

$("refreshModels").addEventListener("click", async () => {
  const btn = $("refreshModels");
  btn.disabled = true;
  btn.textContent = "Loading…";
  try {
    // Save the key first (blank keeps existing) so list_models can use it.
    await call("save_settings", { api_key: $("apiKey").value, model: "" });
    $("apiKey").value = "";
    const models = await call("list_models");
    const sel = $("modelSelect");
    sel.innerHTML = models.map((m) => `<option value="${escapeHtml(m.id)}">${escapeHtml(m.id)}</option>`).join("");
  } catch (e) {
    setSettingsStatus(`Model list failed: ${e.message || e}`, "error");
  } finally {
    btn.disabled = false;
    btn.textContent = "Refresh";
  }
});

// ---- auto-update ----
let pendingUpdate = null;
// Set once install succeeds: the footer/install buttons then relaunch instead
// of re-running install (which would fail with "no update pending").
let installed = false;

function setFoot(state, text) {
  const btn = $("footUpdateBtn");
  if (!btn) return;
  btn.dataset.state = state; // idle | checking | ok | available | error
  btn.disabled = state === "checking";
  $("footUpdateText").textContent = text;
}

async function doInstall(triggerBtn) {
  const btn = triggerBtn;
  const restore = btn ? btn.textContent : null;
  if (btn) { btn.disabled = true; btn.textContent = "Downloading…"; }
  setFoot("checking", "Downloading update…");
  try {
    await call("install_update"); // downloads + installs; resolves when done
    setFoot("checking", "Restarting…");
    if (btn) btn.textContent = "Restarting…";
    // If the relaunch is deferred/blocked, don't leave the UI stuck.
    installed = true;
    setTimeout(() => {
      setFoot("available", "Installed — restart the app");
      if (btn) { btn.disabled = false; btn.textContent = "Restart"; }
    }, 30000);
    try { await call("restart_app"); } catch (_) { /* deferred; fallback covers it */ }
  } catch (e) {
    $("updateBannerText").innerHTML = `<strong>Update failed:</strong> ${escapeHtml(e.message || e)}`;
    setFoot("error", "Update failed — retry");
    if (btn) { btn.disabled = false; btn.textContent = restore || "Retry"; }
  }
}

async function checkForUpdate(silent) {
  const status = $("updateStatus");
  const btn = $("checkUpdateBtn");
  if (!silent) {
    if (btn) { btn.disabled = true; btn.textContent = "Checking…"; }
    if (status) status.textContent = "Checking for updates…";
  }
  setFoot("checking", "Checking…");
  try {
    const res = await call("check_for_update");
    if (res.available) {
      pendingUpdate = res;
      const v = res.version ? `Version ${escapeHtml(res.version)}` : "An update";
      $("updateBannerText").innerHTML =
        `<strong>${v} is available.</strong> ` +
        (res.current ? `You're on ${escapeHtml(res.current)}. ` : "") +
        `Your work is saved to disk; the app will reopen after updating.`;
      $("updateBanner").hidden = false;
      setFoot("available", `Update available: ${res.version} — install`);
      if (!silent && status) status.textContent = `Update available: ${res.version}.`;
    } else {
      pendingUpdate = null;
      setFoot("ok", `Up to date${res.current ? ` · v${res.current}` : ""}`);
      if (!silent && status) status.textContent = `You're on the latest version${res.current ? ` (${res.current})` : ""}.`;
    }
  } catch (e) {
    // A silent startup check stays quiet in the banner (no release / offline);
    // the footer + manual check report the reason.
    setFoot("error", "Check failed — retry");
    if (!silent && status) status.textContent = `Could not check: ${e.message || e}`;
  } finally {
    if (!silent && btn) { btn.disabled = false; btn.textContent = "Check now"; }
  }
}

$("checkUpdateBtn").addEventListener("click", () => checkForUpdate(false));
$("footUpdateBtn").addEventListener("click", () => {
  if (installed) call("restart_app").catch(() => {});
  else if (pendingUpdate) doInstall($("footUpdateBtn"));
  else checkForUpdate(false);
});
$("updateInstallBtn").addEventListener("click", () => {
  if (installed) call("restart_app").catch(() => {});
  else doInstall($("updateInstallBtn"));
});

// Build progress (4.1): live status line while a build/benchmark runs.
if (TAURI && TAURI.event && TAURI.event.listen) {
  TAURI.event.listen("build_progress", (e) => {
    const p = e && e.payload;
    if (!p || !p.detail || !busy) return;
    const bs = $("benchStatus");
    if (bs && !bs.hidden) setBenchStatus(p.detail, "info");
    else setStatus(p.detail, "info");
  });
}

// Recent files (4.2).
async function renderRecent() {
  const el = $("recentFiles");
  if (!el) return;
  let list = [];
  try { list = await call("list_recent"); } catch (_) { return; }
  if (!Array.isArray(list) || !list.length) { el.hidden = true; el.innerHTML = ""; return; }
  el.hidden = false;
  el.innerHTML =
    `<span class="recent-label">Recent</span>` +
    list.map((r) =>
      `<button type="button" class="recent-item" data-path="${escapeHtml(r.path)}" title="${escapeHtml(r.path)}">${escapeHtml(r.label)}</button>`
    ).join("");
  el.querySelectorAll(".recent-item").forEach((b) => {
    b.addEventListener("click", () => call("open_path", { path: b.dataset.path }).catch(() => {}));
  });
}

// ---- startup ----
updateTickerUI();
renderRecent();
updatePeersUI();
initMode();
checkForUpdate(true);

// ---- Web search (Phase 8.4) ----
let searchBusy = false;
let lastHits = [];
let lastQuery = "";

function searchHistoryGet() {
  try { return JSON.parse(localStorage.getItem("searchHistory") || "[]"); } catch { return []; }
}
function searchHistoryPush(q) {
  let h = searchHistoryGet().filter((x) => x !== q);
  h.unshift(q); h = h.slice(0, 8);
  localStorage.setItem("searchHistory", JSON.stringify(h));
  renderSearchHistory();
}
function renderSearchHistory() {
  const el = $("searchHistory"); const h = searchHistoryGet();
  if (!h.length) { el.hidden = true; return; }
  el.hidden = false;
  el.innerHTML = h
    .map((q) => `<button type="button" class="chip" data-q="${escapeHtml(q)}">${escapeHtml(q)}</button>`)
    .join("");
}
function updateSearchUI() {
  $("searchBtn").disabled = !$("searchQuery").value.trim() || searchBusy;
}
async function runSearch(query) {
  if (searchBusy || !query) return;
  searchBusy = true; updateSearchUI();
  $("searchStatus").hidden = true;
  const label = $("searchBtn").querySelector(".btn-label-s");
  if (label) label.textContent = "Searching…";
  try {
    const res = await call("web_search", { query });
    lastHits = res.hits || []; lastQuery = query;
    $("searchBackendPill").textContent = res.backend === "roam" ? "Roam browser" : "basic search";
    searchHistoryPush(query);
    renderSearchHits();
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    $("searchStatus").textContent = `Search failed: ${msg}`;
    $("searchStatus").className = "status error"; $("searchStatus").hidden = false;
  } finally {
    searchBusy = false; updateSearchUI();
    if (label) label.textContent = "Search";
  }
}
function renderSearchHits() {
  $("searchResults").hidden = false;
  $("searchReader").hidden = true;
  $("searchReaderBack").hidden = true;
  $("searchHitList").hidden = false;
  $("readerFind").hidden = true;
  $("searchResTitle").textContent = "Results";
  $("searchResMeta").textContent = `${lastHits.length} results · “${lastQuery}”`;
  const ul = $("searchHitList");
  if (!lastHits.length) {
    ul.innerHTML = `<li class="search-empty">No results — try the Roam browser (Settings) for protected sources.</li>`;
  } else {
    ul.innerHTML = lastHits
      .map((h, i) => `
        <li class="search-hit">
          <a href="#" class="hit-title" data-i="${i}">${escapeHtml(h.title || h.url)}</a>
          <span class="hit-url">${escapeHtml(h.url)}</span>
          ${h.snippet ? `<span class="hit-snippet">${escapeHtml(h.snippet)}</span>` : ""}
        </li>`)
      .join("");
  }
  $("searchResults").scrollIntoView({ behavior: "smooth", block: "nearest" });
}
async function openReader(i) {
  const hit = lastHits[i]; if (!hit) return;
  $("searchHitList").hidden = true;
  $("searchReader").hidden = false;
  $("searchReaderBack").hidden = false;
  $("searchResTitle").textContent = hit.title || "Reading…";
  $("readerUrl").textContent = hit.url; $("readerUrl").title = hit.url; $("readerUrl").dataset.url = hit.url;
  $("readerBody").innerHTML = `<p class="reader-loading">Loading…</p>`;
  $("readerFind").hidden = false; $("readerFind").value = "";
  try {
    const res = await call("read_page", { url: hit.url, query: lastQuery });
    $("readerBody").innerHTML = renderMarkdown(res.text || "") || `<p class="reader-error">Empty page.</p>`;
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    $("readerBody").innerHTML = `<p class="reader-error">Could not read page: ${escapeHtml(msg)}</p>`;
  }
}
function readerFind(term) {
  const body = $("readerBody");
  body.querySelectorAll("mark.find-hit").forEach((m) => m.replaceWith(document.createTextNode(m.textContent)));
  body.normalize();
  if (!term) return;
  const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
  const low = term.toLowerCase(); const nodes = [];
  while (walker.nextNode()) nodes.push(walker.currentNode);
  let first = null;
  for (const n of nodes) {
    const idx = n.nodeValue.toLowerCase().indexOf(low);
    if (idx === -1) continue;
    const range = document.createRange();
    range.setStart(n, idx); range.setEnd(n, idx + term.length);
    const mark = document.createElement("mark"); mark.className = "find-hit";
    try { range.surroundContents(mark); if (!first) first = mark; } catch (_) { /* skip cross-node */ }
  }
  if (first) first.scrollIntoView({ behavior: "smooth", block: "center" });
}

$("searchQuery").addEventListener("input", updateSearchUI);
$("searchBtn").addEventListener("click", () => runSearch($("searchQuery").value.trim()));
$("searchQuery").addEventListener("keydown", (e) => { if (e.key === "Enter") runSearch($("searchQuery").value.trim()); });
$("searchHistory").addEventListener("click", (e) => {
  const b = e.target.closest(".chip");
  if (b) { $("searchQuery").value = b.dataset.q; updateSearchUI(); runSearch(b.dataset.q); }
});
$("searchHitList").addEventListener("click", (e) => {
  const a = e.target.closest(".hit-title");
  if (a) { e.preventDefault(); openReader(parseInt(a.dataset.i, 10)); }
});
$("searchReaderBack").addEventListener("click", renderSearchHits);
$("readerOpen").addEventListener("click", () => {
  const u = $("readerUrl").dataset.url;
  if (u) call("open_url", { url: u }).catch(() => {});
});
$("readerUrl").addEventListener("click", (e) => {
  e.preventDefault();
  const u = $("readerUrl").dataset.url;
  if (u) call("open_url", { url: u }).catch(() => {});
});
$("readerFind").addEventListener("input", (e) => readerFind(e.target.value.trim()));
renderSearchHistory();
updateSearchUI();
