# Release Checklist

> A manual pre-release ritual for finmodel. Run through each step in order before
> cutting a new tag.

---

## 1. Full live tie-out run (API keys required)

The tie-out harness verifies every historical cell of the extraction pipeline against
the source filing PDFs. This requires a working `claude` CLI or Anthropic API key.

```bash
# From the repo root:
python -m tieout.run_tieout
```

**Acceptance:** matches the committed baseline — 339/350 (96.86%) across the 7-company basket, with no *new* mismatches vs `_baseline_wave0.json` (the pytest guard enforces this). The 11 known mismatches are documented extraction-convention targets, not regressions.
The report is written to `tieout/results/_report.md` — scan it for any `FAIL` or `MISMATCH`
lines.

If any cell falls below 100%, investigate before proceeding. Known-skip companies
(SAP.DE, MC.PA) are excluded from the count — they are documented as unsupported
(integrated-report layouts).

```bash
# Single-company fast check when only one changed:
python -m tieout.run_tieout --only ATCO-B.ST
```

---

## 2. Invariant spot-check

Beyond filing tie-out, run the internal verifier and invariant checks:

```bash
python -m pytest tests/ -q -x --tb=short
```

Also run the tie-out regression guard explicitly:

```bash
python -m pytest tests/test_tieout_no_regression.py -q -x --tb=short
```

**Acceptance:** all tests pass. The tie-out guard test compares the current run
against `tieout/results/_baseline_wave0.json` — a regression is an automatic
`FAIL`.

---

## 3. Version bump

Update the version in `pyproject.toml`:

| File | Field |
|------|-------|
| `pyproject.toml` | `version = "X.Y.Z"` |

Update `CHANGELOG.md` with the new version heading and entries.

---

## 4. Commit and push

```bash
git add -A
git commit -m "release: vX.Y.Z"
git tag vX.Y.Z
git push origin main --tags
```

---

## 5. Verify CI

After push, confirm the GitHub Actions CI run completes:

- **pytest** job: all tests pass
- **ruff** job: no lint warnings
- **tie-out guard** job: no regression

If CI fails, fix the issue, bump the patch version, and repeat from step 1.

---

## Reference: CI guard

The CI pipeline (`.github/workflows/ci.yml`) runs on every push and pull request
to `main`. It executes:

1. `pytest tests/` (fast quality gate)
2. `ruff check src/` (lint)
3. `tests/test_tieout_no_regression.py` (baseline comparison without API keys,
   using `FINMODEL_DEV_MOCK=1` to bypass LLM calls)

A red CI is a release blocker.

---

## 6. Desktop app release (auto-update)

The Tauri desktop app (`src-tauri/`) self-updates from GitHub Releases. Builds are
signed with a **minisign** key; the app verifies each update against the `pubkey`
baked into `src-tauri/tauri.conf.json` (`plugins.updater.pubkey`) before installing.

> **Releases live in a PUBLIC repo.** The source repo `finmodel-rust` is private,
> and the updater fetches `latest.json` **unauthenticated** — a private repo 404s
> for clients. So releases (installer + `latest.json`) are published to the public
> **`github.com/Knightwarrior911/finmodel-releases`** repo, which the endpoint in
> `tauri.conf.json` points at (mirrors the `pdf-panda-releases` pattern).

### Signing keys (one-time, already done)
- Keypair generated with `cargo tauri signer generate -w C:\Users\vinit\.tauri\finmodel.key -p ""`.
- **Private key: `C:\Users\vinit\.tauri\finmodel.key` — NEVER commit it.** It lives
  outside the repo. Back it up securely; losing it means no client can ever update
  again. Add it to CI as the secret `TAURI_SIGNING_PRIVATE_KEY` (the file's
  contents, not the path) with `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` empty.
- Public key is committed in `tauri.conf.json` (safe to publish).

### Build the signed installer + updater artifacts
```bash
cd src-tauri
# key as a string; PATH form is NOT honored by this tauri-cli:
TAURI_SIGNING_PRIVATE_KEY="$(cat /c/Users/vinit/.tauri/finmodel.key)" \
TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" \
  cargo tauri build --bundles nsis
```
Produces under `src-tauri/target/release/bundle/nsis/`:
- `finmodel_<version>_x64-setup.exe` — the installer (also the update payload)
- `finmodel_<version>_x64-setup.exe.sig` — the minisign signature (goes in `latest.json`)

### Publish the GitHub Release
1. Bump `version` in `src-tauri/tauri.conf.json` (and `Cargo.toml`) to the new `X.Y.Z`.
2. Create a release tagged `vX.Y.Z` on `github.com/Knightwarrior911/finmodel-releases` (public).
3. Upload two assets: the `-setup.exe` and a `latest.json` (below).

`latest.json` — the updater endpoint
(`…/releases/latest/download/latest.json`) — format:
```json
{
  "version": "0.1.0",
  "notes": "What changed in this release.",
  "pub_date": "2026-07-14T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<paste the ENTIRE contents of finmodel_0.1.0_x64-setup.exe.sig>",
      "url": "https://github.com/Knightwarrior911/finmodel-releases/releases/download/v0.1.0/finmodel_0.1.0_x64-setup.exe"
    }
  }
}
```
`version` MUST be greater (semver) than the installed build or clients won't offer it.

### Client behavior
- On launch the app silently checks the endpoint; if a newer signed build exists it
  shows a "Restart & update" banner. Settings → "Check now" forces a check.
- No release / offline → the silent check stays quiet; the manual check reports it.
- Icons are still pdf-panda placeholders (`src-tauri/icons/`) — rebrand before a
  public release.
