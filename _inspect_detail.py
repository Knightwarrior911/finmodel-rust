"""Detailed inspection of model cache data shape."""
import json
from pathlib import Path

cache_dir = Path("C:/Users/vinit/Documents/financial_model/tieout/results/_modelcache")
fp = "4065a2c76ef95ca6"

f = cache_dir / f"{fp}_ASML_AS.json"
data = json.loads(f.read_text(encoding="utf-8"))

# Check exact value format
is_ = data["income_statement"]
print("=== ASML Income Statement detail ===")
for k, v in list(is_.items())[:4]:
    print(f"  {k}: {v} (type={type(v).__name__})")

# Check a year-keyed entry
rev = is_["revenue"]
print(f"\nrevenue type: {type(rev).__name__}")
if isinstance(rev, dict):
    print(f"revenue keys: {list(rev.keys())}")
    print(f"revenue values: {list(rev.values())}")
else:
    print(f"revenue value: {rev}")

# Full structure
print(f"\nFull data keys: {list(data.keys())}")
print(f"years_found: {data.get('years_found')}")
