"""finmodelaudit: URI launcher — open a source PDF at a specific page in Edge.

Why this exists: Excel hyperlinks to a local PDF are opened via the Windows
shell, which DROPS the `#page=N` fragment, so the PDF always lands on page 1.
Edge honours `#page=N` only when it is launched DIRECTLY with the URL as an
argument. So we register a custom `finmodelaudit:` protocol whose handler is
this script: Excel links carry `finmodelaudit:page=N&path=<abs>` (no `#`, so
Excel keeps the whole thing), the shell hands the full URI to this handler, and
the handler launches the browser directly with `file:///<abs>#page=N`.

Usage:
    python -m src.audit_open --install        # one-time HKCU registration
    python -m src.audit_open "finmodelaudit:page=3&path=C%3A%5C...%5Cx.pdf"

Public API:
    build_uri(path, page) -> str          # the finmodelaudit: link for a cell
    parse_uri(uri) -> (abs_path, page)    # inverse
    open_uri(uri) -> int                  # launch browser at the page
    install() -> str                      # register the protocol (HKCU)
"""
from __future__ import annotations

import os
import subprocess
import sys
from pathlib import Path
from typing import Optional, Tuple
from urllib.parse import quote, unquote, parse_qs

SCHEME = "finmodelaudit"


def build_uri(path: str, page: Optional[int]) -> str:
    """Excel-safe link: finmodelaudit:page=N&path=<percent-encoded-abs-path>.

    No '#' anywhere (Excel splits hyperlinks on '#' and drops the fragment).
    page is 1-based; omitted/None -> page 1.
    """
    abs_path = str(Path(path).resolve())
    pg = page if (page and page >= 1) else 1
    return f"{SCHEME}:page={pg}&path={quote(abs_path, safe='')}"


def parse_uri(uri: str) -> Tuple[str, int]:
    """Inverse of build_uri. Returns (abs_path, page). Tolerates a leading
    scheme and an optional '//' authority."""
    body = uri
    if body.lower().startswith(SCHEME + ":"):
        body = body[len(SCHEME) + 1:]
    body = body.lstrip("/")
    q = parse_qs(body, keep_blank_values=True)
    path = unquote(q.get("path", [""])[0])
    try:
        page = int(q.get("page", ["1"])[0])
    except ValueError:
        page = 1
    return path, max(1, page)


def _find_edge() -> Optional[str]:
    """Locate msedge.exe (App Paths registry, then common install dirs)."""
    try:
        import winreg
        for hive in (winreg.HKEY_CURRENT_USER, winreg.HKEY_LOCAL_MACHINE):
            try:
                with winreg.OpenKey(
                    hive,
                    r"SOFTWARE\Microsoft\Windows\CurrentVersion\App Paths\msedge.exe",
                ) as k:
                    val, _ = winreg.QueryValueEx(k, None)
                    if val and Path(val).exists():
                        return val
            except OSError:
                continue
    except Exception:
        pass
    for env in ("ProgramFiles(x86)", "ProgramFiles", "LOCALAPPDATA"):
        base = os.environ.get(env)
        if not base:
            continue
        cand = Path(base) / "Microsoft" / "Edge" / "Application" / "msedge.exe"
        if cand.exists():
            return str(cand)
    return None


def open_uri(uri: str) -> int:
    """Launch the source PDF at its page. Returns 0 on a launch attempt."""
    path, page = parse_uri(uri)
    if not path or not Path(path).exists():
        return 2
    target = f"{Path(path).resolve().as_uri()}#page={page}"
    edge = _find_edge()
    if edge:
        subprocess.Popen([edge, target])
        return 0
    # Fallback: default handler (won't honour the page, but opens the doc).
    try:
        os.startfile(str(Path(path).resolve()))  # type: ignore[attr-defined]
    except Exception:
        import webbrowser
        webbrowser.open(target)
    return 0


def _pythonw() -> str:
    """pythonw.exe next to the active interpreter (no console flash); fall back
    to python.exe."""
    exe = Path(sys.executable)
    pyw = exe.with_name("pythonw.exe")
    return str(pyw if pyw.exists() else exe)


def install() -> str:
    """Register the finmodelaudit: protocol under HKCU (no admin needed).

    Returns the command line registered.
    """
    import winreg

    handler = str(Path(__file__).resolve())
    cmd = f'"{_pythonw()}" "{handler}" "%1"'
    root = rf"Software\Classes\{SCHEME}"
    with winreg.CreateKey(winreg.HKEY_CURRENT_USER, root) as k:
        winreg.SetValueEx(k, None, 0, winreg.REG_SZ, "URL:finmodel audit source")
        winreg.SetValueEx(k, "URL Protocol", 0, winreg.REG_SZ, "")
    with winreg.CreateKey(
        winreg.HKEY_CURRENT_USER, root + r"\shell\open\command"
    ) as k:
        winreg.SetValueEx(k, None, 0, winreg.REG_SZ, cmd)
    return cmd


def _main(argv: list[str]) -> int:
    if not argv:
        print("usage: python -m src.audit_open --install | <finmodelaudit:...>")
        return 1
    if argv[0] == "--install":
        cmd = install()
        print("Registered finmodelaudit: ->", cmd)
        return 0
    return open_uri(argv[0])


if __name__ == "__main__":
    raise SystemExit(_main(sys.argv[1:]))
