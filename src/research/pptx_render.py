"""
Render .pptx to PNG/PDF for visual verification.

Backends, tried in order when backend="auto":
  1. soffice / LibreOffice headless (--convert-to pdf)  + pdftoppm for PNG
  2. PowerPoint COM via win32com.client                  (Windows only)
  3. Falls back with a clear error if neither is available

Used by the inspect -> plan -> edit -> render -> verify loop. python-pptx
geometry does not match rendered output for text-overflow, font-fallback, and
contrast issues — those only surface in the render.
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Literal, Optional


Backend = Literal["auto", "soffice", "com"]


# ─────────────────────────────────────────────────────────────────────────────
# Backend probes
# ─────────────────────────────────────────────────────────────────────────────

def _find_soffice() -> Optional[str]:
    candidate = shutil.which("soffice") or shutil.which("soffice.exe")
    if candidate:
        return candidate
    # Common Windows install paths
    for p in (
        r"C:\Program Files\LibreOffice\program\soffice.exe",
        r"C:\Program Files (x86)\LibreOffice\program\soffice.exe",
    ):
        if Path(p).exists():
            return p
    return None


def _find_pdftoppm() -> Optional[str]:
    return shutil.which("pdftoppm") or shutil.which("pdftoppm.exe")


def _has_powerpoint_com() -> bool:
    if sys.platform != "win32":
        return False
    try:
        import win32com.client  # noqa: F401
        return True
    except Exception:
        return False


# ─────────────────────────────────────────────────────────────────────────────
# soffice backend
# ─────────────────────────────────────────────────────────────────────────────

def _render_soffice(
    deck_path: Path, out_dir: Path, dpi: int, soffice: str
) -> list[Path]:
    out_dir.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as td:
        td_path = Path(td)
        cmd = [
            soffice, "--headless", "--convert-to", "pdf",
            "--outdir", str(td_path), str(deck_path),
        ]
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=180)
        if result.returncode != 0:
            raise RuntimeError(
                f"soffice failed: {result.stderr.strip() or result.stdout.strip()}"
            )
        pdf_path = td_path / (deck_path.stem + ".pdf")
        if not pdf_path.exists():
            raise RuntimeError(f"soffice produced no PDF at {pdf_path}")

        pdf_dest = out_dir / pdf_path.name
        shutil.copy2(pdf_path, pdf_dest)

        pdftoppm = _find_pdftoppm()
        if not pdftoppm:
            return [pdf_dest]

        png_prefix = out_dir / f"{deck_path.stem}_slide"
        cmd_png = [
            pdftoppm, "-r", str(dpi), "-png",
            str(pdf_dest), str(png_prefix),
        ]
        result = subprocess.run(cmd_png, capture_output=True, text=True, timeout=180)
        if result.returncode != 0:
            return [pdf_dest]

        pngs = sorted(out_dir.glob(f"{deck_path.stem}_slide-*.png"))
        return [pdf_dest] + pngs


# ─────────────────────────────────────────────────────────────────────────────
# PowerPoint COM backend (Windows)
# ─────────────────────────────────────────────────────────────────────────────

def _render_com(deck_path: Path, out_dir: Path, dpi: int) -> list[Path]:
    import win32com.client

    out_dir.mkdir(parents=True, exist_ok=True)
    abs_path = str(deck_path.resolve())
    abs_out = str(out_dir.resolve())

    # ppSaveAsPNG = 18 (export folder of per-slide PNGs)
    # ppSaveAsPDF = 32
    PPSAVE_PNG = 18
    PPSAVE_PDF = 32

    powerpoint = win32com.client.DispatchEx("PowerPoint.Application")
    try:
        # WithWindow=False prevents UI flash
        pres = powerpoint.Presentations.Open(abs_path, WithWindow=False)
        try:
            pdf_dest = Path(abs_out) / (deck_path.stem + ".pdf")
            pres.SaveAs(str(pdf_dest), PPSAVE_PDF)

            png_dir = Path(abs_out) / f"{deck_path.stem}_pngs"
            png_dir.mkdir(parents=True, exist_ok=True)
            pres.SaveAs(str(png_dir), PPSAVE_PNG)

            seen: set[str] = set()
            pngs: list[Path] = []
            for p in sorted(png_dir.glob("*.PNG")) + sorted(png_dir.glob("*.png")):
                key = str(p).lower()
                if key in seen:
                    continue
                seen.add(key)
                pngs.append(p)
            return [pdf_dest] + pngs
        finally:
            pres.Close()
    finally:
        powerpoint.Quit()


# ─────────────────────────────────────────────────────────────────────────────
# Public API
# ─────────────────────────────────────────────────────────────────────────────

def render_deck(
    deck_path: str | Path,
    *,
    out_dir: Optional[str | Path] = None,
    dpi: int = 140,
    backend: Backend = "auto",
) -> list[Path]:
    """
    Render a .pptx to PDF + per-slide PNGs in out_dir. Returns the list of
    written paths (PDF first, PNGs in slide order).
    """
    deck_path = Path(deck_path).resolve()
    if not deck_path.exists():
        raise FileNotFoundError(deck_path)
    out_dir = Path(out_dir) if out_dir else deck_path.parent / "preview"
    out_dir = out_dir.resolve()

    if backend in ("auto", "soffice"):
        soffice = _find_soffice()
        if soffice:
            try:
                return _render_soffice(deck_path, out_dir, dpi, soffice)
            except Exception:
                if backend == "soffice":
                    raise

    if backend in ("auto", "com"):
        if _has_powerpoint_com():
            return _render_com(deck_path, out_dir, dpi)
        if backend == "com":
            raise RuntimeError(
                "PowerPoint COM backend not available. "
                "Requires Windows + PowerPoint + pywin32."
            )

    raise RuntimeError(
        "No render backend available. Install LibreOffice (soffice) and "
        "poppler (pdftoppm), or run on Windows with PowerPoint + pywin32. "
        "Set backend='soffice' or backend='com' to force a specific backend."
    )


# ─────────────────────────────────────────────────────────────────────────────
# CLI
# ─────────────────────────────────────────────────────────────────────────────

def _main() -> None:
    import argparse

    parser = argparse.ArgumentParser(description="Render a .pptx to PDF + PNGs")
    parser.add_argument("path")
    parser.add_argument("--out", default=None)
    parser.add_argument("--dpi", type=int, default=140)
    parser.add_argument(
        "--backend",
        choices=("auto", "soffice", "com"),
        default="auto",
    )
    args = parser.parse_args()

    paths = render_deck(
        args.path, out_dir=args.out, dpi=args.dpi, backend=args.backend
    )
    for p in paths:
        print(p)


if __name__ == "__main__":
    _main()
