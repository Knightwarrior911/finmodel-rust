# Local subscription providers

OpenCode Go and Cursor (via OMP gateway) are **on by default** — no env var before
launch. Opt out with:

```bat
set FINMODEL_DISABLE_SUBSCRIPTION_PROVIDERS=1
```

## OpenCode Go (chat via local OMP gateway)

Finmodel routes OpenCode Go chat through OMP's local gateway at
`http://127.0.0.1:4000/v1`; subscription credentials remain in OpenCode or OMP
and are never copied into finmodel requests.

Finmodel recognizes OpenCode Go as ready only when OMP has an active
`opencode-go` credential in `~/.omp/agent/agent.db`. Authenticate with OMP's
`/login opencode-go`, then click **Connect OpenCode Go**. If authentication is
missing, finmodel opens `https://opencode.ai/auth` and an interactive OMP
terminal. OMP accepts the pasted key and saves it to agent.db; finmodel never reads it.

**Import OpenCode Go key** rechecks OMP's credential database; it never copies
environment variables, OpenCode `auth.json`, or subscription secrets into
finmodel's keyring.

## Cursor (chat via local OMP gateway)

Reuses OMP `~/.omp/agent/agent.db` when you are already logged in.

If OAuth is missing/expired, Settings → **Connect Cursor** launches
`omp auth-broker login cursor` in the background and opens the Cursor browser
login automatically — omp owns the PKCE flow. Settings keeps checking until login
completes, then finmodel starts local `omp auth-broker` + `omp auth-gateway` and
points `base_url` at `http://127.0.0.1:4000/v1`. Once chat-ready, Provider →
Cursor switches the model picker to the live Cursor catalog.
Your saved API key is left unchanged; the gateway uses a dummy bearer with
`--no-auth`.

**Use Cursor** wires the gateway when OAuth is already present.
**Probe Cursor models** lists the complete live catalog through OMP's local
gateway; Provider → Cursor, OpenRouter, and OpenCode Go each keep separate model choices.

Default model: `cursor/claude-4.6-sonnet-medium` (also try `cursor/default`).
Avoid bare `composer-1.5` — it often returns Connect `invalid_argument` /
gateway 502. Smoke body needs a real user message (empty/system-only fails).

## Legacy

`FINMODEL_ENABLE_SUBSCRIPTION_PROVIDERS=0|false|off` still disables (compat with
the earlier opt-in gate).
