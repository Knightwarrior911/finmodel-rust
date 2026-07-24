# Setting Up Providers on a New Machine

## What you need first

- **OMP** installed (the provider gateway that manages logins)
  ```bash
  # Install via bun
  bun install -g omp
  ```
- **finmodel** desktop app installed (the `.exe` from the release)

---

## Connecting Cursor (for chat/synthesis)

1. Open the finmodel desktop app
2. Go to **Settings** → **Personal subscription providers**
3. Click **Connect Cursor**
4. A browser window opens to cursor.com — sign in with your Cursor account
5. Once signed in, the app detects the login and wires the local gateway automatically
6. You'll see **"Cursor live gateway verified"** in Settings

Cursor handles plain chat and synthesis. It does **not** support finmodel's research tools (financial analysis, model building, etc.) — that's by design.

---

## Connecting OpenCode Go (for tools + full agent)

1. In Settings → **Personal subscription providers**, click **Connect OpenCode Go**
2. A **terminal window** opens — this is the OMP login prompt
3. In that terminal:
   - It opens `opencode.ai/auth` in your browser
   - Sign in with your OpenCode account
   - Copy the API key from the browser page
   - Paste it into the terminal and press Enter
4. Back in the app, click **Use existing OpenCode Go login** (or it auto-connects)
5. You'll see **"OpenCode Go live gateway verified"** in Settings

OpenCode Go is the one with **full tool support** — financial analysis, model building, research, filing reads, everything.

---

## Which one to pick?

| Provider | Chat | Research Tools | Financial Models |
|----------|------|----------------|-----------------|
| **OpenCode Go** | ✅ | ✅ | ✅ |
| **Cursor** | ✅ | ❌ | ❌ |
| **OpenRouter** | ✅ | ✅ | ✅ (if key set) |

If you want the full finmodel experience (building models, reading filings, running financial analysis), use **OpenCode Go**. If you just want quick chat/summaries, Cursor is fine.

---

## If something goes wrong

- **"No Cursor OAuth"** → Click Connect Cursor again and complete the browser login
- **"No OpenCode Go credential"** → Click Connect OpenCode Go and complete the terminal login
- **"Gateway not verified"** → Restart the app; OMP services start automatically on first use
- **Tools not working** → Make sure you're on OpenCode Go, not Cursor. Settings shows which provider is active.

---

## Quick test

Once connected, try asking in the chat:
> "Get the latest revenue for Apple"

If you're on OpenCode Go, it will call the `get_financials` tool and return actual SEC EDGAR numbers. If you're on Cursor, it'll answer from memory (which finmodel doesn't trust — it wants tool-sourced data).
