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
  const card = document.getElementById("settingsModal").querySelector(".modal-card");
  card.dispatchEvent(new window.KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
  await tick();
  assert.equal(document.getElementById("settingsModal").hidden, true, "closed on Escape");
  assert.ok(!document.getElementById("app").hasAttribute("inert"), "inert cleared");
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
