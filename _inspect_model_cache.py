"""Inspect model cache structure for snapshot generation feasibility."""
import json
from pathlib import Path

cache_dir = Path("C:/Users/vinit/Documents/financial_model/tieout/results/_modelcache")
fp = "4065a2c76ef95ca6"

for f in sorted(cache_dir.glob(f"{fp}_*.json")):
    ticker = f.stem.replace(f"{fp}_", "")
    data = json.loads(f.read_text(encoding="utf-8"))
    print(f"=== {ticker} ===")
    print(f"  years_found: {data.get('years_found')}")
    is_ = data.get("income_statement", {})
    bs = data.get("balance_sheet", {})
    cfs = data.get("cash_flow_statement", {})
    print(f"  income_statement: {len(is_)} keys, samples={list(is_.keys())[:3]}")
    print(f"  balance_sheet:    {len(bs)} keys, samples={list(bs.keys())[:3]}")
    print(f"  cash_flow:        {len(cfs)} keys, samples={list(cfs.keys())[:3]}")
    # Check if years are in values
    if is_:
        first_key = list(is_.keys())[0]
        val = is_[first_key]
        print(f"    year keys in {first_key}: {list(val.keys()) if isinstance(val, dict) else 'scalar'}")
    print()
