#!/usr/bin/env python3
"""Turn the external uprobe maps into counts and per-site receiver words.

+4 words are ShapeIds only for shaped receivers. For array/nonobject sites,
use IC reason 10 to identify that these are length/layout words instead.
"""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import re

p = argparse.ArgumentParser()
p.add_argument("files", nargs="+", type=Path)
opts = p.parse_args()
results = {}
for path in opts.files:
    raw = path.read_text()
    shapes = defaultdict(dict)
    for site, shape, count in re.findall(r"@shapes\[(\d+), (\d+)\]: (\d+)", raw):
        shapes[int(site)][int(shape)] = int(count)
    sites = []
    for site, name, count in re.findall(r"@slow\[(\d+), (.*?)\]: (\d+)", raw):
        site = int(site)
        sites.append({"site": site, "key": name, "slow": int(count),
                      "distinct_receiver_words": len(shapes[site]),
                      "receiver_words": sorted(shapes[site].items(), key=lambda row: -row[1])})
    err = path.with_suffix(".err")
    stats = {}
    if err.exists():
        for line in err.read_text().splitlines():
            if line.startswith("[method-site]"):
                stats = {key: int(value) for key, value in re.findall(r"([\w.]+)=(\d+)", line)}
    results[str(path)] = {
        "counts": {key: int(value) for key, value in re.findall(r"@counts\[(.*?)\]: (\d+)", raw)},
        "reasons": {int(key): int(value) for key, value in re.findall(r"@reasons\[(\d+)\]: (\d+)", raw)},
        "cache_allocated": {key: int(value) for key, value in re.findall(r"@cache_state\[(.*?)\]: (\d+)", raw)},
        "method_site_stats": stats,
        "sites": sorted(sites, key=lambda site: -site["slow"]),
    }
print(json.dumps(results, indent=2))
