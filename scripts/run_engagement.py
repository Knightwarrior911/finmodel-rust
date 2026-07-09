#!/usr/bin/env python3
"""One-command engagement flow for financial model.

Creates a dated client folder, runs the full model pipeline, and packages
the deliverable (model xlsx, branding config, sources appendix, optional deck).

Usage::

    python scripts/run_engagement.py --ticker AAPL
    python scripts/run_engagement.py --ticker AAPL --output-dir ./engagements/ --deck
"""

from __future__ import annotations

import argparse
import datetime
import json
import os
import shutil
import subprocess
import sys

# The repo root is the parent of the scripts/ directory.
_REPO_ROOT = os.path.normpath(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
)


def _resolve(*parts: str) -> str:
    return os.path.join(_REPO_ROOT, *parts)


def _find_extraction_cache(ticker: str) -> str | None:
    """Locate the extraction-cache JSON for *ticker* (may be . or _ delimited)."""
    variants = [
        ticker.upper(),
        ticker,
        ticker.replace(".", "_").replace("-", "_"),
        ticker.upper().replace(".", "_").replace("-", "_"),
    ]
    seen: set[str] = set()
    for v in variants:
        if v in seen:
            continue
        seen.add(v)
        p = _resolve("extraction_cache", f"{v}.json")
        if os.path.exists(p):
            return p
    return None


def main() -> None:
    parser = argparse.ArgumentParser(
        description=(
            "One-command engagement flow — run the full pipeline and "
            "package the deliverable into a dated client folder."
        )
    )
    parser.add_argument(
        "--ticker",
        required=True,
        help="Company ticker (e.g. AAPL, MSFT)",
    )
    parser.add_argument(
        "--output-dir",
        default="./engagements/",
        help="Root output directory for engagements (default: ./engagements/)",
    )
    parser.add_argument(
        "--deck",
        action="store_true",
        help=(
            "Also build a PowerPoint summary deck alongside the Excel model. "
            "Requires python-pptx; may fail without it."
        ),
    )
    args = parser.parse_args()

    ticker = args.ticker.upper()
    date_str = datetime.date.today().isoformat()

    # Resolve the output root.
    if os.path.isabs(args.output_dir):
        out_root = args.output_dir
    else:
        out_root = _resolve(args.output_dir)

    engagement_dir = os.path.join(out_root, ticker, date_str)
    os.makedirs(engagement_dir, exist_ok=True)

    model_path = os.path.join(engagement_dir, f"{ticker}_model.xlsx")

    # ── Pipeline invocation ──────────────────────────────────────────────
    cmd = [
        sys.executable,
        "-m",
        "src.cli",
        "--ticker",
        ticker,
        "--direct",
        "--output",
        model_path,
    ]
    if args.deck:
        cmd.append("--deck")

    print(f"\u2554\u2550\u2550 ENGAGEMENT FLOW \u2014 {ticker} \u2550\u2550\u2557")
    print(f"  Output folder:  {engagement_dir}")
    print(f"  Model:          {model_path}")
    if args.deck:
        deck_path = model_path.replace(".xlsx", "_Summary.pptx")
        print(f"  Deck:           {deck_path}")
    else:
        print(f"  Deck:           skipped (use --deck to enable)")
    print(f"  Command:        {' '.join(cmd)}")
    print()

    result = subprocess.run(cmd, cwd=_REPO_ROOT)
    if result.returncode != 0:
        print(f"ERROR: Pipeline failed with exit code {result.returncode}")
        sys.exit(result.returncode)

    files_created: list[str] = [model_path]

    # ── Copy branding config ────────────────────────────────────────────
    branding_src = _resolve("config", "branding.yaml")
    if os.path.exists(branding_src):
        branding_dst = os.path.join(engagement_dir, "branding.yaml")
        shutil.copy2(branding_src, branding_dst)
        files_created.append(branding_dst)
        print(f"  \u2713 Copied branding config")
    else:
        print(f"  \u26a0 Branding config not found at {branding_src}")

    # ── Generate sources appendix ───────────────────────────────────────
    cache_path = _find_extraction_cache(ticker)
    if cache_path and os.path.exists(cache_path):
        cache_dst = os.path.join(engagement_dir, "extraction_cache.json")
        shutil.copy2(cache_path, cache_dst)
        files_created.append(cache_dst)
        print(f"  \u2713 Copied extraction cache")

        # Generate a human-readable sources report markdown.
        try:
            with open(cache_path, encoding="utf-8") as f:
                cache = json.load(f)

            # Ensure the repo root is on sys.path for the import.
            _added = _REPO_ROOT not in sys.path
            if _added:
                sys.path.insert(0, _REPO_ROOT)
            from src.sources_report import build_sources_report

            report = build_sources_report(cache)
            sources_md = os.path.join(engagement_dir, "SOURCES.md")
            with open(sources_md, "w", encoding="utf-8") as f:
                f.write(report)
                f.write("\n")
            files_created.append(sources_md)
            print(f"  \u2713 Generated sources appendix (SOURCES.md)")
        except Exception as e:
            print(f"  \u26a0 Sources report generation skipped: {e}")
    else:
        print(f"  \u26a0 Extraction cache not found \u2014 sources appendix omitted")

    # ── Also copy deck if generated ─────────────────────────────────────
    if args.deck:
        deck_path = model_path.replace(".xlsx", "_Summary.pptx")
        if os.path.exists(deck_path):
            files_created.append(deck_path)

    # ── Summary ─────────────────────────────────────────────────────────
    print()
    print(f"\u2554\u2550\u2550 SUMMARY \u2014 {ticker} \u2550\u2550\u2557")
    print(f"  Engagement folder: {engagement_dir}")
    for fp in sorted(files_created, key=lambda p: os.path.basename(p)):
        if os.path.exists(fp):
            size = os.path.getsize(fp)
            rel = os.path.relpath(fp, engagement_dir)
            print(f"    {rel:50s} {size:>8,} bytes")
    print(f"  Total files: {len(files_created)}")
    print()


if __name__ == "__main__":
    main()
