"""Build click-to-source hyperlink strings (no rendering, no files).

A located PDF citation becomes a file:///…#page=N URI that modern viewers
(Edge default on Win11, Adobe, browsers) open at the right page. Non-PDF
sources (market data) pass through their URL. Page is the contract; bbox is
recorded elsewhere for a future highlight and not used here.

Public API:
    make_audit_link(pdf_path, *, page_index=None, url=None) -> str | None
"""
from __future__ import annotations

from pathlib import Path
from typing import Optional


def make_audit_link(
    pdf_path: Optional[str],
    *,
    page_index: Optional[int] = None,
    url: Optional[str] = None,
) -> Optional[str]:
    """Return a hyperlink string for a sourced number, or None.

    - pdf_path + page_index -> file:///abs/doc.pdf#page=(page_index+1)
    - pdf_path only         -> file:///abs/doc.pdf  (opens page 1)
    - url (market_data)     -> url unchanged
    - nothing               -> None

    page_index is 0-based (PyMuPDF convention); the emitted #page fragment is
    1-based (PDF-viewer convention).
    """
    if pdf_path:
        uri = Path(pdf_path).resolve().as_uri()  # file:///C:/...
        if page_index is not None and page_index >= 0:
            return f"{uri}#page={page_index + 1}"
        return uri
    if url:
        return url
    return None
