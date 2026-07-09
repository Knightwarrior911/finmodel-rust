from pathlib import Path
import json

ec = Path("C:/Users/vinit/Documents/financial_model/extraction_cache")
items = list(ec.iterdir())
print(f"{len(items)} items in extraction_cache")
for x in items[:20]:
    sz = x.stat().st_size
    print(f"  {x.name}  ({sz:,} bytes)")
    # peek inside if json
    if x.suffix == ".json" and sz < 50000:
        try:
            data = json.loads(x.read_text(encoding="utf-8"))
            if isinstance(data, dict):
                print(f"    keys: {list(data.keys())[:6]}")
        except: pass
