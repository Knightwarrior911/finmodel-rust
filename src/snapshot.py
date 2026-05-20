"""Snapshot renderer — turn a CellProvenance into a PNG of the source PDF page
with a yellow translucent rectangle drawn over the located number.

Public API:
    render_snapshot(prov, out_dir) -> Path     full-page snapshot
"""
from __future__ import annotations

from pathlib import Path
from typing import Optional
import io
import re

from .provenance import CellProvenance


DEFAULT_DPI = 144
YELLOW_RGBA = (255, 255, 0, 90)         # translucent fill
BORDER_RGBA = (200, 160, 0, 255)         # opaque amber border
BORDER_WIDTH_PX = 2
BBOX_PAD_PT = 1.5                        # PDF-point padding before highlight


def _safe_filename(s: str) -> str:
    """Make string safe for Windows + POSIX filenames."""
    return re.sub(r"[^A-Za-z0-9._-]+", "_", s).strip("_") or "cell"


def render_snapshot(
    prov: CellProvenance,
    out_dir: str | Path,
    *,
    dpi: int = DEFAULT_DPI,
    ticker: Optional[str] = None,
) -> Path:
    """Render a page snapshot with the located number highlighted.

    prov.pdf_path must exist on disk. If prov.bbox is None (low_confidence),
    the page is rendered without a highlight rectangle.

    Output: out_dir/{ticker_or_blank}/{key}_{period}.png
    Returns the absolute Path to the written PNG.
    """
    import fitz  # PyMuPDF
    from PIL import Image, ImageDraw

    pdf_path = Path(prov.pdf_path)
    if not pdf_path.exists():
        raise FileNotFoundError(f"PDF not found: {pdf_path}")

    doc = fitz.open(str(pdf_path))
    try:
        if not (0 <= prov.page_index < doc.page_count):
            raise IndexError(
                f"page_index {prov.page_index} out of range for {pdf_path} "
                f"(pages={doc.page_count})"
            )
        page = doc[prov.page_index]
        zoom = dpi / 72.0
        mat = fitz.Matrix(zoom, zoom)
        pix = page.get_pixmap(matrix=mat, alpha=False)
        img_bytes = pix.tobytes("png")
    finally:
        doc.close()

    base = Image.open(io.BytesIO(img_bytes)).convert("RGBA")

    if prov.bbox is not None:
        x0, y0, x1, y1 = prov.bbox
        # Pad
        x0 -= BBOX_PAD_PT; y0 -= BBOX_PAD_PT
        x1 += BBOX_PAD_PT; y1 += BBOX_PAD_PT
        # PDF points → pixels
        sx0 = x0 * zoom; sy0 = y0 * zoom
        sx1 = x1 * zoom; sy1 = y1 * zoom
        # Clamp
        w, h = base.size
        sx0 = max(0, min(w - 1, sx0)); sx1 = max(0, min(w, sx1))
        sy0 = max(0, min(h - 1, sy0)); sy1 = max(0, min(h, sy1))

        overlay = Image.new("RGBA", base.size, (0, 0, 0, 0))
        draw = ImageDraw.Draw(overlay)
        draw.rectangle([sx0, sy0, sx1, sy1], fill=YELLOW_RGBA)
        # Border
        for k in range(BORDER_WIDTH_PX):
            draw.rectangle(
                [sx0 - k, sy0 - k, sx1 + k, sy1 + k],
                outline=BORDER_RGBA,
            )
        base = Image.alpha_composite(base, overlay)

    base = base.convert("RGB")

    out_root = Path(out_dir)
    if ticker:
        out_root = out_root / _safe_filename(ticker)
    out_root.mkdir(parents=True, exist_ok=True)

    name = _safe_filename(f"{prov.key}_{prov.period or '0'}")
    out_path = out_root / f"{name}.png"
    base.save(out_path, "PNG", optimize=True)
    return out_path.resolve()
