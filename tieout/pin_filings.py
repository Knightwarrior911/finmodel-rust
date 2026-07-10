"""Pin ONE source annual-report PDF per company, once, then immutable.

Independent of src/ (the loop must not be able to change what gets pinned).
ATCO is already pinned from disk. Others: best-effort DuckDuckGo HTML search
+ direct download. Discovery is deliberately NOT the thing under test — if it
fails, the company is skipped and the run continues. A human can always drop
filings/<ticker>/annual_report.pdf manually and it will be picked up.
"""
import re
import sys
import time
from urllib.parse import quote, urlparse, unquote

import requests

from tieout.config import ticker_filings_dir

_UA = {
    "User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) "
                  "AppleWebKit/537.36 (KHTML, like Gecko) "
                  "Chrome/124.0 Safari/537.36",
    "Accept": "application/pdf,application/octet-stream,*/*",
}


def pinned_pdf_path(row: dict):
    """Return Path to the pinned PDF if present, else None."""
    d = ticker_filings_dir(row["ticker"])
    name = row.get("pinned") or "annual_report.pdf"
    p = d / name
    if p.exists() and p.stat().st_size > 200_000:
        return p
    # any pdf a human dropped in
    for cand in sorted(d.glob("*.pdf")):
        if cand.stat().st_size > 200_000:
            return cand
    return None


def _ddg_pdf_links(query: str):
    url = f"https://html.duckduckgo.com/html/?q={quote(query)}"
    r = requests.get(url, headers=_UA, timeout=25)
    r.raise_for_status()
    raw = re.findall(r'href="(https?://[^"]+)"', r.text)
    links = []
    for h in raw:
        h = unquote(h)
        m = re.search(r"uddg=([^&]+)", h)
        if m:
            h = unquote(m.group(1))
        if h.lower().split("?")[0].endswith(".pdf"):
            links.append(h)
    # de-dup, keep order
    seen, out = set(), []
    for h in links:
        if h not in seen:
            seen.add(h)
            out.append(h)
    return out


def _looks_like_company(url: str, company: str) -> bool:
    host = urlparse(url).netloc.lower()
    toks = [t for t in re.split(r"[^a-z]+", company.lower())
            if len(t) >= 4 and t not in ("group", "holding", "moet")]
    return any(t in host or t in url.lower() for t in toks)


def _download(url: str, dest):
    pu = urlparse(url)
    headers = {**_UA, "Referer": f"{pu.scheme}://{pu.netloc}/"}
    r = requests.get(url, headers=headers, timeout=90, stream=True)
    r.raise_for_status()
    # ONE iterator only: calling r.iter_content() twice with different chunk
    # sizes on a streamed response can silently truncate (some CDNs drop the
    # connection after the first read), which produced sub-200KB files.
    it = r.iter_content(65536)
    first = next(it, b"")
    if not first.startswith(b"%PDF"):
        raise ValueError("not a PDF")
    with open(dest, "wb") as f:
        f.write(first)
        for chunk in it:
            f.write(chunk)
    if dest.stat().st_size < 200_000:
        dest.unlink(missing_ok=True)
        raise ValueError("PDF too small")


def ensure_pinned(row: dict):
    """Return Path to a pinned PDF, or None if it could not be obtained."""
    existing = pinned_pdf_path(row)
    if existing:
        return existing

    d = ticker_filings_dir(row["ticker"])
    dest = d / "annual_report.pdf"

    # Curated direct filing URL — the reliable path. Search is only a fallback
    # for an unattended overnight run (DDG HTML is bot-blocked).
    curated = row.get("url")
    if curated:
        for attempt in range(3):
            try:
                print(f"  [pin] curated {curated[:90]}", file=sys.stderr)
                _download(curated, dest)
                print(f"  [pin] OK -> {dest}", file=sys.stderr)
                return dest
            except Exception as e:  # noqa: BLE001
                print(f"  [pin] curated attempt {attempt+1} failed: {e}",
                      file=sys.stderr)
                time.sleep(4 * (attempt + 1))

    queries = [
        f"{row['search']} {y} filetype:pdf" for y in (2024, 2023)
    ] + [f"{row['company']} consolidated financial statements pdf"]
    for q in queries:
        try:
            links = _ddg_pdf_links(q)
        except Exception as e:  # noqa: BLE001
            print(f"  [pin] search failed ({q!r}): {e}", file=sys.stderr)
            time.sleep(3)
            continue
        ranked = sorted(
            links, key=lambda u: (not _looks_like_company(u, row["company"]),
                                  "annual" not in u.lower()))
        for u in ranked[:5]:
            try:
                print(f"  [pin] trying {u[:90]}", file=sys.stderr)
                _download(u, dest)
                print(f"  [pin] OK -> {dest}", file=sys.stderr)
                return dest
            except Exception as e:  # noqa: BLE001
                print(f"  [pin] reject: {e}", file=sys.stderr)
                time.sleep(2)
    return None


if __name__ == "__main__":
    from tieout.config import BASKET
    only = sys.argv[1] if len(sys.argv) > 1 else None
    for r in BASKET:
        if only and r["ticker"] != only:
            continue
        p = ensure_pinned(r)
        print(f"{r['ticker']:12s} {'PINNED ' + str(p) if p else 'MISSING'}")
