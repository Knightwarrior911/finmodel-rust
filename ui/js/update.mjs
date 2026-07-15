// update.mjs — auto-updater controls (footer button + availability banner).

import { $, call, escapeHtml } from "./core.mjs";

let pendingUpdate = null;
let installed = false;

function setFoot(state, text) {
  const btn = $("footUpdateBtn");
  if (!btn) return;
  btn.dataset.state = state; // idle | checking | ok | available | error
  btn.disabled = state === "checking";
  const t = $("footUpdateText");
  if (t) t.textContent = text;
}

async function doInstall(triggerBtn) {
  const btn = triggerBtn;
  const restore = btn ? btn.textContent : null;
  if (btn) {
    btn.disabled = true;
    btn.textContent = "Downloading…";
  }
  setFoot("checking", "Downloading update…");
  try {
    await call("install_update");
    setFoot("checking", "Restarting…");
    if (btn) btn.textContent = "Restarting…";
    installed = true;
    setTimeout(() => {
      setFoot("available", "Installed — restart");
      if (btn) {
        btn.disabled = false;
        btn.textContent = "Restart";
      }
    }, 30000);
    try {
      await call("restart_app");
    } catch (_) {
      /* deferred; fallback covers it */
    }
  } catch (e) {
    const bt = $("updateBannerText");
    if (bt) bt.innerHTML = `<strong>Update failed:</strong> ${escapeHtml(e.message || e)}`;
    setFoot("error", "Update failed — retry");
    if (btn) {
      btn.disabled = false;
      btn.textContent = restore || "Retry";
    }
  }
}

async function checkForUpdate(silent) {
  setFoot("checking", "Checking…");
  try {
    const res = await call("check_for_update");
    if (res.available) {
      pendingUpdate = res;
      const v = res.version ? `Version ${escapeHtml(res.version)}` : "An update";
      const bt = $("updateBannerText");
      if (bt)
        bt.innerHTML =
          `<strong>${v} is available.</strong> ` +
          (res.current ? `You're on ${escapeHtml(res.current)}. ` : "") +
          `Your work is saved to disk; the app will reopen after updating.`;
      const banner = $("updateBanner");
      if (banner) banner.hidden = false;
      setFoot("available", `Update ${res.version} — install`);
    } else {
      pendingUpdate = null;
      setFoot("ok", `Up to date${res.current ? ` · v${res.current}` : ""}`);
    }
  } catch (_) {
    setFoot("error", "Check failed — retry");
  }
}

export function initUpdate() {
  const foot = $("footUpdateBtn");
  if (foot)
    foot.addEventListener("click", () => {
      if (installed) call("restart_app").catch(() => {});
      else if (pendingUpdate) doInstall(foot);
      else checkForUpdate(false);
    });
  const inst = $("updateInstallBtn");
  if (inst)
    inst.addEventListener("click", () => {
      if (installed) call("restart_app").catch(() => {});
      else doInstall(inst);
    });
  const dismiss = $("updateDismiss");
  if (dismiss)
    dismiss.addEventListener("click", () => {
      const banner = $("updateBanner");
      if (banner) banner.hidden = true;
    });
  checkForUpdate(true);
}
