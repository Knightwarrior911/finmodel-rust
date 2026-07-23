"""Independent CLI transport for the tie-out instrument.

This is a deliberate, self-contained copy of the answer-key CLI call — it does
NOT import ``src.extractor``. Claude remains the default transport; Codex can be
selected explicitly with ``FINMODEL_TIEOUT_TRANSPORT=codex`` for authenticated
ChatGPT users. The ground-truth pass must stay fixed so the metric remains an
independent check.
"""
import os
import subprocess
import sys
import tempfile
import time

_TRANSPORT = os.environ.get("FINMODEL_TIEOUT_TRANSPORT", "claude").strip().lower()
if _TRANSPORT not in {"claude", "codex"}:
    raise ValueError(
        "FINMODEL_TIEOUT_TRANSPORT must be 'claude' or 'codex', "
        f"got {_TRANSPORT!r}"
    )

# Keep the existing Claude default unchanged. Codex gets a plain model alias
# unless the caller explicitly overrides FINMODEL_TIEOUT_MODEL.
_MODEL = os.environ.get(
    "FINMODEL_TIEOUT_MODEL", "gpt-5.5" if _TRANSPORT == "codex" else "opus"
)


class LLMStall(RuntimeError):
    """Raised when the selected CLI fails or stalls after retries."""


def complete(system_text: str, user_text: str, *, timeout: int = 600,
             retries: int = 2) -> str:
    """Make one answer-key call and retry non-zero, empty, or timed-out calls."""
    last_err = ""
    for attempt in range(retries + 1):
        try:
            out = _call_once(system_text, user_text, timeout=timeout)
            if out.strip():
                return _strip_fences(out)
            last_err = "empty output"
        except subprocess.TimeoutExpired:
            last_err = f"timeout after {timeout}s"
        except Exception as e:  # noqa: BLE001 - transport is best-effort
            last_err = f"{type(e).__name__}: {e}"
        if attempt < retries:
            time.sleep(5 * (attempt + 1))
    raise LLMStall(
        f"{_TRANSPORT} CLI failed after {retries + 1} attempts: {last_err}"
    )


def _call_once(system_text: str, user_text: str, *, timeout: int) -> str:
    if _TRANSPORT == "codex":
        return _call_codex(system_text, user_text, timeout=timeout)
    return _call_claude(system_text, user_text, timeout=timeout)


def _call_claude(system_text: str, user_text: str, *, timeout: int) -> str:
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".txt", delete=False, encoding="utf-8"
    ) as sf:
        sf.write(system_text)
        sys_file = sf.name
    try:
        args = [
            "--model", _MODEL,
            "--system-prompt-file", sys_file,
            "--output-format", "text",
            "-p", "Process the piped input per the system instructions and "
                  "return only the requested JSON.",
        ]
        out, err, rc = _run_cli("claude", args, user_text, timeout=timeout)
    finally:
        os.unlink(sys_file)
    if rc != 0:
        raise RuntimeError(f"claude rc={rc}: {err[:300]}")
    return out.strip()


def _call_codex(system_text: str, user_text: str, *, timeout: int) -> str:
    with tempfile.NamedTemporaryFile(
        mode="w", suffix=".txt", delete=False, encoding="utf-8"
    ) as answer:
        answer_file = answer.name
    prompt = (
        system_text
        + "\n\nProcess the following piped input per those instructions and "
          "return only the requested JSON.\n\n"
        + user_text
    )
    try:
        args = [
            "exec",
            "--ephemeral",
            "--sandbox", "read-only",
            "--skip-git-repo-check",
            "--model", _MODEL,
            "--output-last-message", answer_file,
            "-",
        ]
        _out, err, rc = _run_cli("codex", args, prompt, timeout=timeout)
        if rc != 0:
            raise RuntimeError(f"codex rc={rc}: {err[:300]}")
        try:
            with open(answer_file, encoding="utf-8") as output:
                return output.read().strip()
        except OSError as e:
            raise RuntimeError(f"codex output file unavailable: {e}") from e
    finally:
        try:
            os.unlink(answer_file)
        except FileNotFoundError:
            pass


def _run_cli(command: str, args: list[str], input_text: str, *, timeout: int):
    if sys.platform == "win32":
        proc = subprocess.Popen(
            ["cmd", "/c", command] + args,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        out_b, err_b = proc.communicate(
            input=input_text.encode("utf-8"), timeout=timeout
        )
        return (
            out_b.decode("utf-8", errors="replace"),
            err_b.decode("utf-8", errors="replace"),
            proc.returncode,
        )
    result = subprocess.run(
        [command] + args,
        input=input_text,
        capture_output=True,
        text=True,
        timeout=timeout,
        encoding="utf-8",
        errors="replace",
    )
    return result.stdout, result.stderr, result.returncode


def _strip_fences(out: str) -> str:
    out = out.strip()
    if out.startswith("```"):
        lines = out.split("\n")
        inner = lines[1:]
        if inner and inner[-1].strip() == "```":
            inner = inner[:-1]
        out = "\n".join(inner).strip()
    if out.startswith("json"):
        out = out[4:].strip()
    return out
