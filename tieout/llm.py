"""Independent `claude -p` transport for the tie-out instrument.

This is a deliberate, self-contained COPY of the Claude CLI call — it does NOT
import src.extractor. The loop is allowed to rewrite src.extractor's transport;
the ground-truth pass must stay fixed, otherwise the metric is no longer an
independent check. Keep this file dumb and stable.
"""
import os
import subprocess
import sys
import tempfile
import time

# Explicit model for the answer-key transport. The global Claude Code default
# (e.g. an aliased "opus[1m]" beta) can fail headless `-p` invocations with
# rc=1; pinning a plain alias here keeps the instrument runnable regardless of
# the user's interactive settings. Override with FINMODEL_TIEOUT_MODEL.
_MODEL = os.environ.get("FINMODEL_TIEOUT_MODEL", "opus")


class LLMStall(RuntimeError):
    """Raised when the claude CLI fails or stalls after retries."""


def complete(system_text: str, user_text: str, *, timeout: int = 600,
             retries: int = 2) -> str:
    """One-shot claude -p call. Retries on non-zero exit / empty / timeout."""
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
    raise LLMStall(f"claude CLI failed after {retries + 1} attempts: {last_err}")


def _call_once(system_text: str, user_text: str, *, timeout: int) -> str:
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
        if sys.platform == "win32":
            proc = subprocess.Popen(
                ["cmd", "/c", "claude"] + args,
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
            )
            out_b, err_b = proc.communicate(
                input=user_text.encode("utf-8"), timeout=timeout
            )
            out = out_b.decode("utf-8", errors="replace")
            err = err_b.decode("utf-8", errors="replace")
            rc = proc.returncode
        else:
            r = subprocess.run(
                ["claude"] + args,
                input=user_text,
                capture_output=True,
                text=True,
                timeout=timeout,
                encoding="utf-8",
                errors="replace",
            )
            out, err, rc = r.stdout, r.stderr, r.returncode
    finally:
        os.unlink(sys_file)

    if rc != 0:
        raise RuntimeError(f"claude rc={rc}: {err[:300]}")
    return out.strip()


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
