# Third-party notices

Finmodel's agentic runtime **reimplements behavior and interface contracts** from
the projects below in native Rust / vanilla JavaScript. No upstream source is
copied verbatim; these projects were studied as behavioral/specification
references (plan Task 9.4). Their licenses are acknowledged here.

## Behavioral references (studied, clean-room reimplemented)

- **Oh My Pi** — MIT License. © Mario Zechner and Can Bölük.
  Referenced patterns: UI-agnostic recursive agent loop, normalized provider
  event stream, typed tool results, resumable sessions, context compaction.
  https://github.com/can1357/oh-my-pi

- **OpenClaw** — MIT License. © OpenClaw Foundation.
  Referenced patterns: injected loop hooks (before/after tool call, steering /
  follow-up boundaries), retry/failover classification, idle/cost breakers,
  durable commitments, heartbeat/schedule triggers.
  Excluded (never ported): gateways, chat channels, DM pairing, companion nodes,
  voice, remote approval routing.
  https://github.com/openclaw/openclaw

- **Hermes Agent** — MIT License. © Nous Research.
  Referenced patterns: conditional progressive tool disclosure
  (`tool_search`/`tool_describe`/`tool_call`), full tool-result preservation with
  bounded previews + opaque handles, at-least-once async delegation, layered
  prompt assembly + tail-protected compaction, deterministic skill lifecycle.
  https://github.com/NousResearch/hermes-agent

## Notes
- Coding-agent surfaces (shell/edit/git/LSP/debugger/worktree) and any
  chat-channel/gateway code are intentionally **not** ported.
- The finance engines, SQLite store, provider integration, and all agent
  lifecycle code are original to this repository.
