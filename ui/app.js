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
  $("benchBtn").disabled = list.length === 0;
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
  setStatus(`Building model for ${ticker}… reading the filing, projecting 5 years, writing Excel.`, "info");
  try {
    const model = await call("build_model", { ticker });
    lastModel = model;
    $("results").hidden = false;
    $("resTitle").textContent = `${model.ticker}  ·  ${model.currency}`;
    $("resMeta").textContent =
      `Source: ${model.source}  ·  Historical ${model.hist_periods.join(", ")}  ·  Projected ${model.proj_periods.join(", ")}`;
    renderTable();
    clearStatus();
    $("results").scrollIntoView({ behavior: "smooth", block: "nearest" });
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    setStatus(`Could not build ${ticker}: ${msg}`, "error");
    $("results").hidden = true;
  } finally {
    $("buildBtn").disabled = false;
    if (label) label.textContent = "Build model";
    updateTickerUI();
  }
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
  const list = parsePeers(tickers);
  setBenchStatus(`Fetching SEC filings for ${list.length} companies…`, "info");
  try {
    const res = await call("benchmark_peers", { tickers });
    lastBench = res;
    $("benchResults").hidden = false;
    $("benchTitle").textContent = `Peer Benchmark  ·  ${res.count} of ${res.requested}`;
    const fails = (res.failed || []).map((f) => `${f.ticker} (${f.why})`).join("; ");
    $("benchMeta").textContent = fails
      ? `SEC EDGAR XBRL · skipped: ${fails}`
      : `SEC EDGAR XBRL · ${res.count} peers · every number cites an exact filing fact`;
    renderBench();
    $("benchStatus").hidden = true;
    $("benchResults").scrollIntoView({ behavior: "smooth", block: "nearest" });
  } catch (e) {
    const msg = e && e.message ? e.message : String(e);
    setBenchStatus(`Could not benchmark: ${msg}`, "error");
    $("benchResults").hidden = true;
  } finally {
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

async function openSettings() {
  try {
    const s = await call("load_settings");
    $("keyStatus").textContent = s.has_key
      ? "A key is saved. Leave blank to keep it."
      : "No key set — offline demo tickers only.";
    const sel = $("modelSelect");
    if (s.model) sel.innerHTML = `<option value="${escapeHtml(s.model)}">${escapeHtml(s.model)}</option>`;
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
  try {
    await call("save_settings", { api_key, model });
    $("apiKey").value = "";
    closeSettings();
    await initMode();
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
    sel.innerHTML = models.map((m) => `<option value="${escapeHtml(m.id)}">${escapeHtml(m.id)}</option>`).join("");
  } catch (e) {
    setStatus(`Model list failed: ${e.message || e}`, "error");
  } finally {
    btn.disabled = false;
    btn.textContent = "Refresh";
  }
});

// ---- auto-update ----
let pendingUpdate = null;

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
    await call("install_update"); // on success: downloads, installs, relaunches
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
  if (pendingUpdate) doInstall($("footUpdateBtn"));
  else checkForUpdate(false);
});
$("updateInstallBtn").addEventListener("click", () => doInstall($("updateInstallBtn")));

// ---- startup ----
updateTickerUI();
updatePeersUI();
initMode();
checkForUpdate(true);
