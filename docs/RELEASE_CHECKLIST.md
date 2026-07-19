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

The shipped product is the Tauri desktop app, so its version is the source of
truth. For a **desktop release**, bump BOTH of these to the new `X.Y.Z` — they
MUST stay in lockstep or the updater compares against the wrong number
(`check_for_update` reads `app.package_info().version`, i.e. `src-tauri/Cargo.toml`,
against the endpoint's `latest.json`):

| File (REQUIRED, lockstep) | Field |
|------|-------|
| `src-tauri/tauri.conf.json` | `"version": "X.Y.Z"` |
| `src-tauri/Cargo.toml` | `version = "X.Y.Z"` |

`pyproject.toml` (the legacy Python core) is versioned **independently** — bump it
only for a Python-core release, not on every desktop release. It is not part of the
desktop lockstep and does not drive the updater.

Update `CHANGELOG.md` with the new version heading and entries.

---
## 4. Commit and push (no tag yet)

Push the release commit WITHOUT a tag — the immutable tag is created only after
the release gates are green (step 5), so a tag never points at an unverified
commit:

```bash
git add -A
git commit -m "release: vX.Y.Z"
git push origin master
```

---

## 5. Verify CI

After push, confirm the GitHub Actions CI run completes. `.github/workflows/ci.yml`
runs on push / PR to `master` with least-privilege (`permissions: contents: read`):

- **test** — `pytest` (mock LLM) for the legacy Python core
- **ruff** — Python lint
- **rust** — `cargo build/test --workspace` on `finmodel-core`, then the
  **research-eval hard gate** (`cargo test -p fm-research --test research_eval`)
- **app** (windows-latest) — `cargo test --manifest-path src-tauri/Cargo.toml --lib`
  (exercises the Tauri IPC command layer on the shipped WebView2 target OS)
- **ui** — `npm ci` + `npm test` (jsdom mock-DOM regression suite)

If CI fails, fix the issue on `master` (no tag exists yet) and repeat step 4.
A red CI is a release blocker.

Once **every** gate is green, create the immutable tag on the verified commit and
push it:

```bash
git tag vX.Y.Z
git push origin vX.Y.Z
```

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
  "version": "X.Y.Z",
  "notes": "What changed in this release.",
  "pub_date": "2026-01-01T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<paste the ENTIRE contents of finmodel_X.Y.Z_x64-setup.exe.sig>",
      "url": "https://github.com/Knightwarrior911/finmodel-releases/releases/download/vX.Y.Z/finmodel_X.Y.Z_x64-setup.exe"
    }
  }
}
```
`version` MUST be greater (semver) than the installed build or clients won't offer it.

> **Signature newline pitfall (bit us in v0.9.10).** The `.sig` file ends with a
> trailing newline. If it is pasted or shell-substituted verbatim (``
> preserves inner newlines when quoted; naive scripts append \n), the client
> updater fails with `Invalid symbol 10, offset 420` at install time — AFTER
> download, on every machine. Strip it when building `latest.json`:
> `node -e "...readFileSync(sig,'utf8').replace(/[\r\n]+$/,'')"` (or `tr -d '\r\n'`).
> Verify BEFORE publishing: the signature string must be exactly 420 chars ending
> in `=` with no embedded/trailing newline. Also note: replacing a release asset
> does NOT reliably bust GitHub's download CDN — verify the served bytes changed
> (step 7), and if stale, re-upload with changed content (different byte size).

### Client behavior
- On launch the app silently checks the endpoint; if a newer signed build exists it
  shows a "Restart & update" banner. Settings → "Check now" forces a check.
- No release / offline → the silent check stays quiet; the manual check reports it.
- Icons are still pdf-panda placeholders (`src-tauri/icons/`) — rebrand before a
  public release.

---

## 7. Post-release verification

Immediately after publishing, confirm the update channel actually serves the new
build (the endpoint is unauthenticated, so `curl` sees exactly what clients see):

```bash
REL=Knightwarrior911/finmodel-releases
# What every client sees (endpoint is unauthenticated):
curl.exe -sL "https://github.com/$REL/releases/latest/download/latest.json" -o latest.json
cat latest.json    # assert .version == the just-released X.Y.Z
# The installer URL must return 200:
URL=$(node -e "console.log(require('./latest.json').platforms['windows-x86_64'].url)")
curl.exe -sIL "$URL" | head -1
# `gh release view` with no tag views the release the /latest/ endpoint serves;
# its tag must be the one just released:
gh release view --repo "$REL" --json tagName --jq .tagName   # expect vX.Y.Z
```

A pre-existing client (older version) should show the "Restart & update" banner on
next launch or on Settings → "Check now"; a same-version client stays quiet.

---

## 8. Rollback (a bad release shipped)

**The updater never downgrades** — Tauri only offers a build whose `latest.json`
`version` is *greater* (semver) than the installed one. So there is no "revert to
the previous version" that reaches clients already on the bad build. Rollback is
therefore two moves. Set concrete tags first (dotted semver is not shell-arithmetic;
the operator fills these in), and use them verbatim below:

```bash
REL=Knightwarrior911/finmodel-releases
GOOD_TAG=v1.2.2          # last-good release
BAD_TAG=v1.2.3           # the release to withdraw
HOTFIX_TAG=v1.2.4        # the roll-forward release
HOTFIX_VERSION=1.2.4     # HOTFIX_TAG without the leading 'v'
```

1. **Stop the bleed (new installs + not-yet-updated clients).** The `/releases/latest/`
   endpoint resolves to whichever release is flagged *Latest*, so re-point it at the
   last-good release — its `latest.json` already advertises the good version, so no
   client is offered the bad build:

   ```bash
   # Re-flag the last-good release as Latest (endpoint now serves its good latest.json):
   gh release edit "$GOOD_TAG" --repo "$REL" --latest
   # Confirm the endpoint now serves the last-good release (not the bad tag):
   gh release view --repo "$REL" --json tagName --jq .tagName   # expect $GOOD_TAG
   # Optional: stop offering the bad installer as a manual download too.
   gh release delete-asset "$BAD_TAG" "finmodel_${BAD_TAG#v}_x64-setup.exe" --repo "$REL" --yes
   ```
   Clients still on the good version are now offered nothing (correct — they stay
   good); no further clients auto-update onto the bad build.

2. **Fix forward (clients already on the bad build).** A downgrade cannot be pushed,
   so ship a NEW higher patch `$HOTFIX_TAG` that reverts the regression by running
   the normal release path (bump step 3 → commit/push step 4 → green CI + push the
   tag step 5). The tag already exists on the CI-verified commit, so publish it as
   the new Latest with `--verify-tag` (which refuses to fabricate a missing tag from
   the branch tip):

   ```bash
   gh release create "$HOTFIX_TAG" --repo "$REL" --latest --verify-tag \
     "finmodel_${HOTFIX_VERSION}_x64-setup.exe" latest.json
   ```
   Every client — good and bad — is then offered the hotfix.

**Verify the rollback:** re-run step 7 against the endpoint and confirm
`latest.json.version` is the intended target (last-good for move 1, the hotfix for
move 2); a VM/second machine still on the bad build must be offered the hotfix (or,
for move 1 alone, offered nothing rather than the bad build again).

> Never delete the signing key or a published tag to "undo" a release — tags are
> immutable for clients that cached them, and a lost key means no client can ever
> update again (see Signing keys). Roll forward, don't erase.
