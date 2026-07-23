import importlib
from pathlib import Path

from tieout import llm


def test_claude_remains_default_transport(monkeypatch):
    monkeypatch.delenv("FINMODEL_TIEOUT_TRANSPORT", raising=False)
    monkeypatch.delenv("FINMODEL_TIEOUT_MODEL", raising=False)
    module = importlib.reload(llm)
    assert module._TRANSPORT == "claude"
    assert module._MODEL == "opus"


def test_codex_transport_uses_read_only_ephemeral_exec(monkeypatch):
    monkeypatch.setenv("FINMODEL_TIEOUT_TRANSPORT", "codex")
    monkeypatch.setenv("FINMODEL_TIEOUT_MODEL", "gpt-5.5")
    module = importlib.reload(llm)
    captured = {}

    def fake_run(command, args, input_text, *, timeout):
        captured.update(command=command, args=args, input_text=input_text, timeout=timeout)
        output_path = Path(args[args.index("--output-last-message") + 1])
        output_path.write_text('{"ok":true}', encoding="utf-8")
        return "codex event output", "", 0

    monkeypatch.setattr(module, "_run_cli", fake_run)
    assert module._call_once("SYSTEM", "USER", timeout=7) == '{"ok":true}'
    assert captured["command"] == "codex"
    assert captured["args"][:3] == ["exec", "--ephemeral", "--sandbox"]
    assert captured["args"][captured["args"].index("--sandbox") + 1] == "read-only"
    assert captured["args"][captured["args"].index("--model") + 1] == "gpt-5.5"
    assert "SYSTEM" in captured["input_text"]
    assert "USER" in captured["input_text"]


def test_extractor_dispatches_explicit_codex_transport(monkeypatch):
    from src import extractor

    monkeypatch.setenv("FINMODEL_TIEOUT_TRANSPORT", "codex")
    monkeypatch.setenv("DEEPSEEK_API_KEY", "ignored")
    monkeypatch.setenv("ANTHROPIC_API_KEY", "ignored")
    calls = {}

    def fake_codex(system_text, user_text):
        calls.update(system=system_text, user=user_text)
        return "codex-result"

    monkeypatch.setattr(extractor, "_llm_complete_via_codex", fake_codex)
    assert extractor._llm_complete("SYSTEM", "USER", 99) == "codex-result"
    assert calls == {"system": "SYSTEM", "user": "USER"}
