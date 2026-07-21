# Build / Test / Release

## Tests (what CI runs — `.github/workflows/ci.yml`)
- Core engine: `cd finmodel-core && cargo test --workspace`
- Research eval gate: `cd finmodel-core && cargo test -p fm-research --test research_eval -- --nocapture`
- Answer-quality gate: in the same `research_eval` binary — `answer_quality_meets_committed_baseline`
  (scores grounded answers vs `tests/baselines/quality_v1.json`) + `cli_ingestion_path_scores_committed_gold_fixture`.
- Sweep CLI (scores model×prompt answer artifacts offline):
  `cargo run -p fm-research --example quality_sweep -- <artifacts.json> <gold_answers.json> [min_mean_floor]`
  (exit 1 below floor, 2 on bad input); grading = `quality_eval::run_sweep`.
- App lib: `cargo test --manifest-path src-tauri/Cargo.toml --lib`  (≈389 tests)
- UI: `cd ui && npm ci --no-audit --no-fund && npm test`  (jsdom, ≈208 tests)

Fast local loops:
- App lib one test: `cd src-tauri && cargo test --lib <name>`
- UI one file: `cd ui && node --test tests/cards.test.mjs`
- Type-check only: `cd src-tauri && cargo check --lib` (expect `0` errors AND `0` warnings before shipping)

Ignored live tests hit the real model/network (run only when online):
`cargo test --lib data_room_live_smoke -- --ignored --nocapture`

## Build the desktop app (NSIS installer, Windows)
`cd src-tauri && CI=true cargo tauri build --bundles nsis` — UNSIGNED, local smoke only.
- Produces `src-tauri/target/release/bundle/nsis/finmodel_<ver>_x64-setup.exe`.
- For a RELEASE, sign DURING the build by passing the updater key IN-ENV (authoritative path;
  the tauri-cli honors the key CONTENTS, not a path):
  `TAURI_SIGNING_PRIVATE_KEY="$(cat < /c/Users/vinit/.tauri/finmodel.key)" \`
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" CI=true cargo tauri build --bundles nsis`
  → also emits `finmodel_<ver>_x64-setup.exe.sig` (the minisign sig for `latest.json`).
  NEVER commit or echo the key; read it via `< redirection` (the bare arg form fails on this shell).

## Release ritual (see docs/RELEASE_CHECKLIST.md for the authoritative version)
Source repo `Knightwarrior911/finmodel-rust` is PRIVATE; releases go to PUBLIC
`Knightwarrior911/finmodel-releases`. Updater endpoint:
`https://github.com/Knightwarrior911/finmodel-releases/releases/latest/download/latest.json`

1. **Version lockstep** — bump BOTH `src-tauri/Cargo.toml` and `src-tauri/tauri.conf.json`
   to the same `X.Y.Z`; `cargo check`/build refreshes `src-tauri/Cargo.lock`.
2. Prepend a `## vX.Y.Z - <date> - <headline>` block to `CHANGELOG.md` (warm, user-facing copy).
3. Commit (`release: vX.Y.Z - …`), `git push origin master` — **NO tag yet**.
4. Wait for CI green on the pushed SHA: `gh run list -L 1` → `gh run watch <id> --exit-status`.
   A red CI is a release blocker — fix on master and re-push before tagging.
5. **Only after CI is green**, tag the verified commit and push it (a tag never points at
   unverified code): `git tag -a vX.Y.Z -m "…" && git push origin vX.Y.Z`.
6. Build the SIGNED installer (build section above — key in-env; emits exe + .sig).
7. Write `latest.json` next to the exe: `{version, notes, pub_date, platforms:{"windows-x86_64":
   {signature:<.sig contents>, url:<download URL for this tag>}}}`. STRIP the `.sig` trailing
   newline (bit us in v0.9.10 → `Invalid symbol 10`): the signature MUST be exactly 420 chars
   ending in `=`, no embedded/trailing newline.
8. `gh release create vX.Y.Z --repo Knightwarrior911/finmodel-releases --title "finmodel X.Y.Z" --notes "…" --latest <setup.exe> latest.json`.
9. **Verify the endpoint**: curl the latest.json download URL → `version` == X.Y.Z, sig len 420,
   and `curl -sIL <installer url>` → `HTTP/1.1 200`.
10. Append a fresh HANDOVER block to `docs/HANDOVER_LOG.md` (or note in commit) — do NOT bloat root CLAUDE.md.

## OpenRouter key
Stored in the OS credential store, not `settings.json`. In code: `commands::secrets::get_api_key()`
(service `finmodel`, account `openrouter_api_key`). Tests read it via that fn.
