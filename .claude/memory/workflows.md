# Build / Test / Release

## Tests (what CI runs — `.github/workflows/ci.yml`)
- Core engine: `cd finmodel-core && cargo test --workspace`
- Research eval gate: `cd finmodel-core && cargo test -p fm-research --test research_eval -- --nocapture`
- App lib: `cargo test --manifest-path src-tauri/Cargo.toml --lib`  (≈379 tests)
- UI: `cd ui && npm ci --no-audit --no-fund && npm test`  (jsdom, ≈205 tests)

Fast local loops:
- App lib one test: `cd src-tauri && cargo test --lib <name>`
- UI one file: `cd ui && node --test tests/cards.test.mjs`
- Type-check only: `cd src-tauri && cargo check --lib` (expect `0` errors AND `0` warnings before shipping)

Ignored live tests hit the real model/network (run only when online):
`cargo test --lib data_room_live_smoke -- --ignored --nocapture`

## Build the desktop app (NSIS installer, Windows)
`cd src-tauri && CI=true cargo tauri build --bundles nsis`
- Produces `src-tauri/target/release/bundle/nsis/finmodel_<ver>_x64-setup.exe`.
- The build's own signing step FAILS by design (no `TAURI_SIGNING_PRIVATE_KEY` env) — sign manually below.

## Release ritual (see docs/RELEASE_CHECKLIST.md for the authoritative version)
Source repo `Knightwarrior911/finmodel-rust` is PRIVATE; releases go to PUBLIC
`Knightwarrior911/finmodel-releases`. Updater endpoint:
`https://github.com/Knightwarrior911/finmodel-releases/releases/latest/download/latest.json`

1. **Version lockstep** — bump BOTH `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
   to the same `X.Y.Z`; `cargo check` to refresh `src-tauri/Cargo.lock`.
2. Prepend a `## vX.Y.Z - <date> - <headline>` block to `CHANGELOG.md` (warm, user-facing copy).
3. Commit (`release: vX.Y.Z - …`), `git push origin master`.
4. Wait for CI green on the pushed SHA: `gh run watch <id> --exit-status`.
5. Build NSIS (above). Then **sign** the installer with the Tauri updater signer:
   `cd src-tauri && cargo tauri signer sign -f <signing-key-path> <setup.exe>`
   The key location and its passphrase are local secrets — see `docs/RELEASE_CHECKLIST.md`
   / your local config, never hard-code them here. A signature is 420 chars.
6. Write `latest.json` next to the exe: `{version, notes, pub_date, platforms:{"windows-x86_64":{signature:<.sig contents>, url:<download URL for this tag>}}}`.
7. `git tag -a vX.Y.Z -m "…" && git push origin vX.Y.Z`.
8. `gh release create vX.Y.Z --repo Knightwarrior911/finmodel-releases --title "finmodel X.Y.Z" --notes "…" --latest <setup.exe> latest.json`.
9. **Verify the endpoint**: curl the latest.json download URL → `version` == X.Y.Z, sig len 420,
   and `curl -sIL <installer url>` → `HTTP/1.1 200`.
10. Append a fresh HANDOVER block to `docs/HANDOVER_LOG.md` (or note in commit) — do NOT bloat root CLAUDE.md.

## OpenRouter key
Stored in the OS credential store, not `settings.json`. In code: `commands::secrets::get_api_key()`
(service `finmodel`, account `openrouter_api_key`). Tests read it via that fn.
