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
let lastModel = null;
let activeStmt = "income_statement";

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
      return `<th class="${isLbl ? "lbl" : "num"}${isProj ? " proj" : ""}">${h}</th>`;
    })
    .join("");
  thead.innerHTML = `<tr>${head}</tr>`;

  const keys = Array.from(new Set([...Object.keys(hist), ...Object.keys(proj)])).sort();
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
      return `<tr><td class="lbl">${prettyKey(k)}</td>${cells.join("")}</tr>`;
    })
    .join("");
}

async function build(ticker) {
  clearStatus();
  $("buildBtn").disabled = true;
  const label = $("buildBtn").querySelector(".btn-label");
  if (label) label.textContent = "Building…";
  setStatus(`Building model for ${ticker}…`, "info");
  try {
    const model = await call("build_model", { ticker });
    lastModel = model;
    $("results").hidden = false;
    $("resTitle").textContent = `${model.ticker}  ·  ${model.currency}`;
    $("resMeta").textContent =
      `Source: ${model.source}  ·  Historical ${model.hist_periods.join(", ")}  ·  Projected ${model.proj_periods.join(", ")}`;
    renderTable();
    clearStatus();
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    setStatus(`Could not build: ${msg}`, "error");
    $("results").hidden = true;
  } finally {
    $("buildBtn").disabled = false;
    if (label) label.textContent = "Build model";
  }
}

// ---- events ----
$("buildBtn").addEventListener("click", () => {
  const t = $("ticker").value.trim();
  if (t) build(t);
});
$("ticker").addEventListener("keydown", (e) => {
  if (e.key === "Enter" && $("ticker").value.trim()) build($("ticker").value.trim());
});
$("demoChips").addEventListener("click", (e) => {
  const b = e.target.closest(".chip");
  if (b) {
    $("ticker").value = b.dataset.t;
    build(b.dataset.t);
  }
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
  thead.innerHTML = "<tr>" + BENCH_COLS.map((c) => `<th>${c[1]}</th>`).join("") + "</tr>";
  tbody.innerHTML = lastBench.rows
    .map((r) => "<tr>" + BENCH_COLS.map((c) => `<td>${c[2](r)}</td>`).join("") + "</tr>")
    .join("");
}

async function benchmark(tickers) {
  $("benchBtn").disabled = true;
  const label = $("benchBtn").querySelector(".btn-label-b");
  if (label) label.textContent = "Benchmarking…";
  setBenchStatus(`Fetching filings for ${tickers}…`, "info");
  try {
    const res = await call("benchmark_peers", { tickers });
    lastBench = res;
    $("benchResults").hidden = false;
    $("benchTitle").textContent = `Peer Benchmark  ·  ${res.count}/${res.requested}`;
    const fails = (res.failed || []).map((f) => `${f.ticker} (${f.why})`).join("; ");
    $("benchMeta").textContent = fails
      ? `SEC EDGAR XBRL · skipped: ${fails}`
      : `SEC EDGAR XBRL · ${res.count} peers · numbers cite exact us-gaap facts`;
    renderBench();
    $("benchStatus").hidden = true;
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    setBenchStatus(`Could not benchmark: ${msg}`, "error");
    $("benchResults").hidden = true;
  } finally {
    $("benchBtn").disabled = false;
    if (label) label.textContent = "Benchmark";
  }
}

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

async function openSettings() {
  try {
    const s = await call("load_settings");
    $("keyStatus").textContent = s.has_key
      ? "A key is saved. Leave blank to keep it."
      : "No key set — offline demo tickers only.";
    const sel = $("modelSelect");
    if (s.model) sel.innerHTML = `<option value="${s.model}">${s.model}</option>`;
  } catch (_) {
    /* offline / first launch */
  }
  modal.hidden = false;
  $("apiKey").focus();
}

function closeSettings() {
  modal.hidden = true;
}

$("settingsBtn").addEventListener("click", openSettings);
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
  try {
    await call("save_settings", { api_key, model });
    $("apiKey").value = "";
    closeSettings();
    setStatus("Settings saved.", "info");
    setTimeout(clearStatus, 2000);
  } catch (e) {
    setStatus(`Save failed: ${e.message || e}`, "error");
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
    sel.innerHTML = models.map((m) => `<option value="${m.id}">${m.id}</option>`).join("");
  } catch (e) {
    setStatus(`Model list failed: ${e.message || e}`, "error");
  } finally {
    btn.disabled = false;
    btn.textContent = "Refresh";
  }
});
