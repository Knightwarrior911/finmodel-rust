# Local subscription providers

OpenCode Go and Cursor (via OMP gateway) are **on by default** — no env var before
launch. Opt out with:

```bat
set FINMODEL_DISABLE_SUBSCRIPTION_PROVIDERS=1
```

## OpenCode Go (chat-ready)

Base URL: `https://opencode.ai/zen/go/v1` (OpenAI-compatible).

On first launch, if the keyring is empty, finmodel auto-imports a key from (in
order):

1. `OPENCODE_API_KEY`
2. OpenCode `auth.json` (`opencode-go` / `opencode`) under
   `%USERPROFILE%\.local\share\opencode\`
3. OMP `~/.omp/agent/agent.db` after `/login opencode-go`

If none of those exist, Settings → **Connect OpenCode Go** opens
`https://opencode.ai/auth` so you can copy a key, paste it into **API key**,
and Save. **Import OpenCode Go key** still pulls from the local sources above
without opening the browser.

## Cursor (chat via local OMP gateway)

Reuses OMP `~/.omp/agent/agent.db` when you are already logged in.

If OAuth is missing/expired, Settings → **Connect Cursor** (or Provider →
Cursor) launches `omp auth-broker login cursor` in the background and opens the
Cursor browser login automatically — omp owns the PKCE flow. Settings keeps
checking until login completes, then finmodel starts local `omp auth-broker` +
`omp auth-gateway` and points `base_url` at `http://127.0.0.1:4000/v1`.
Your saved API key is left unchanged; the gateway uses a dummy bearer with
`--no-auth`.

**Use Cursor** wires the gateway when OAuth is already present.
**Probe Cursor models** lists usable ids via `omp models cursor`.

Default model: `cursor/claude-4.6-sonnet-medium` (also try `cursor/default`).
Avoid bare `composer-1.5` — it often returns Connect `invalid_argument` /
gateway 502. Smoke body needs a real user message (empty/system-only fails).

## Legacy

`FINMODEL_ENABLE_SUBSCRIPTION_PROVIDERS=0|false|off` still disables (compat with
the earlier opt-in gate).
