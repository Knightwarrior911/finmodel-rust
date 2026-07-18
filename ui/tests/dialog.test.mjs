import { test } from "node:test";
import assert from "node:assert/strict";
import { setupDom, importModule, tick } from "./harness.mjs";

async function bootSettings() {
  const ctx = setupDom();
  ctx.invokeHandlers.load_settings = async () => ({
    has_key: true,
    model: "m",
    edgar_contact: "",
    out_dir: "",
    mcp_command: "",
    mcp_args: [],
    version: "0.4.0",
    model_capability: null,
  });
  ctx.invokeHandlers.list_models = async () => ({ models: [] });
  const settings = await importModule("settings.mjs");
  settings.initSettings({ onSaved: () => {} });
  return { ctx, settings };
}

test("opening settings marks background inert and traps focus", async () => {
  await bootSettings();
  document.dispatchEvent(new window.CustomEvent("open-settings"));
  await tick();
  await tick();
  const modal = document.getElementById("settingsModal");
  assert.equal(modal.hidden, false, "modal shown");
  // Background siblings (the app shell) are inert while the dialog is open.
  const app = document.getElementById("app");
  assert.ok(app.hasAttribute("inert"), "app shell inert behind dialog");
  // Dialog exposes modal semantics.
  const card = modal.querySelector(".modal-card");
  assert.equal(card.getAttribute("role"), "dialog");
  assert.equal(card.getAttribute("aria-modal"), "true");
  assert.equal(card.getAttribute("aria-labelledby"), "settingsTitle");
});

test("Escape closes settings and clears inert", async () => {
  await bootSettings();
  document.dispatchEvent(new window.CustomEvent("open-settings"));
  await tick();
  await tick();
  const card = document
    .getElementById("settingsModal")
    .querySelector(".modal-card");
  card.dispatchEvent(
    new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
  );
  await tick();
  assert.equal(
    document.getElementById("settingsModal").hidden,
    true,
    "closed on Escape",
  );
  assert.ok(
    !document.getElementById("app").hasAttribute("inert"),
    "inert cleared",
  );
});

test("focus returns to the opener after closing settings", async () => {
  await bootSettings();
  const opener = document.getElementById("settingsBtn");
  opener.focus();
  opener.click();
  await tick();
  await tick();
  // Close via the close button.
  document.getElementById("settingsClose").click();
  await tick();
  assert.equal(document.activeElement, opener, "focus returned to opener");
});

test("role profiles populate from load_settings (Task 1.5)", async () => {
  const { ctx } = await bootSettings();
  ctx.invokeHandlers.load_settings = async () => ({
    has_key: true,
    model: "m",
    edgar_contact: "",
    out_dir: "",
    mcp_command: "",
    mcp_args: [],
    version: "0.4.0",
    model_capability: null,
    model_profiles: {
      worker: {
        provider_base: "https://api.deepseek.com/v1",
        model: "deepseek-chat",
        credential_ref: "ds_key",
      },
      verifier: null,
      fallbacks: [],
    },
  });
  document.dispatchEvent(new window.CustomEvent("open-settings"));
  await tick();
  await tick();
  assert.equal(document.getElementById("workerModel").value, "deepseek-chat");
  assert.equal(
    document.getElementById("workerProviderBase").value,
    "https://api.deepseek.com/v1",
  );
  assert.equal(document.getElementById("workerCredentialRef").value, "ds_key");
  // An absent verifier role leaves its inputs blank (orchestrator-only).
  assert.equal(document.getElementById("verifierModel").value, "");
});

test("saving sends model_profiles built from the role inputs (Task 1.5)", async () => {
  const { ctx } = await bootSettings();
  ctx.invokeHandlers.save_settings = async () => ({ ok: true });
  ctx.invokeHandlers.test_model = async () => ({ model_id: "m" });
  document.dispatchEvent(new window.CustomEvent("open-settings"));
  await tick();
  await tick();
  document.getElementById("workerProviderBase").value = "https://api.x.ai/v1";
  document.getElementById("workerModel").value = "grok-2";
  document.getElementById("workerCredentialRef").value = "xai_key";
  document.getElementById("saveSettings").click();
  await tick();
  await tick();
  const saved = ctx.invokeLog.find((c) => c.name === "save_settings");
  assert.ok(saved, "save_settings invoked");
  assert.equal(saved.payload.model_profiles.worker.model, "grok-2");
  assert.equal(
    saved.payload.model_profiles.worker.provider_base,
    "https://api.x.ai/v1",
  );
  assert.equal(saved.payload.model_profiles.worker.credential_ref, "xai_key");
  // A blank verifier role serializes as null, not an empty profile.
  assert.equal(saved.payload.model_profiles.verifier, null);
});

test("skills list surfaces lifecycle state + restore for stale skills (Task 7.2)", async () => {
  const { ctx } = await bootSettings();
  ctx.invokeHandlers.skills_list = async () => [
    {
      name: "earnings-snapshot",
      description: "d",
      state: "stale",
      use_count: 2,
      source_version: 1,
    },
    {
      name: "fresh-skill",
      description: "d2",
      state: "active",
      use_count: 0,
      source_version: 1,
    },
  ];
  ctx.invokeHandlers.skill_restore = async () => {};
  document.dispatchEvent(new window.CustomEvent("open-settings"));
  await tick();
  await tick();
  const list = document.getElementById("skillsList");
  // The stale skill shows a state badge; the active one does not.
  assert.match(list.textContent, /\(stale\)/);
  assert.doesNotMatch(list.textContent, /\(active\)/);
  // Only the stale skill gets a Restore control.
  const restores = list.querySelectorAll("button[data-restore]");
  assert.equal(restores.length, 1);
  assert.equal(restores[0].dataset.restore, "earnings-snapshot");
  // Clicking Restore invokes skill_restore for that skill (reversible).
  restores[0].click();
  await tick();
  const restored = ctx.invokeLog.find((c) => c.name === "skill_restore");
  assert.ok(restored, "skill_restore invoked");
  assert.equal(restored.payload.name, "earnings-snapshot");
});
