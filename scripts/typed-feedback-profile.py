#!/usr/bin/env python3
"""Join a same-build site catalog and runtime trace into an advisory replay profile."""
import argparse
import copy
import json
from pathlib import Path


def make_profile(catalog, trace):
    if catalog.get("schema_version") != 1:
        raise ValueError("unsupported site catalog schema_version (expected 1)")
    rows = {}
    for row in trace["sites"]:
        key = (row["site_id"], row["function"], row["kind"], row["operation"])
        if key in rows:
            raise ValueError(f"duplicate runtime trace site: {key}")
        rows[key] = row
    profile = copy.deepcopy(catalog)
    selected = 0
    for module in profile["modules"]:
        sites = []
        for site in module["sites"]:
            key = (site["site_id"], site["function"], site["kind"], site["operation"])
            row = rows.get(key)
            if row is None or not row.get("observed_count", 0):
                continue
            observations = row.get("observed_kinds", [])
            # Consume only stable, pointer-free numeric observations. Runtime
            # addresses, shape IDs, and method/closure identities never replay.
            if (site["kind"] == "array_element" and site["operation"] == "array[index]"
                    and observations and all(
                        obs.get("source") == "array"
                        and obs.get("heap_type") == "array"
                        and obs.get("array_access") == "indexed_in_bounds"
                        and obs.get("array_element_kind") in ("number", "int32")
                        for obs in observations)):
                site["observation_kind"] = "numeric_array_element"
                sites.append(site)
                selected += 1
        module["sites"] = sorted(sites, key=lambda site: site["site_id"])
    profile["modules"].sort(key=lambda module: module["identity"]["module"])
    if not selected:
        raise ValueError("trace contains no supported numeric array-read observations; compile with PERRY_TYPED_FEEDBACK=1 and exercise an array[index] read")
    return profile


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sites", required=True, type=Path, help="--typed-feedback-sites catalog from the instrumented build")
    parser.add_argument("--trace", required=True, type=Path, help="typed-feedback-trace.json from that same build")
    parser.add_argument("-o", "--output", required=True, type=Path)
    args = parser.parse_args()
    try:
        profile = make_profile(json.loads(args.sites.read_text()), json.loads(args.trace.read_text()))
        args.output.write_text(json.dumps(profile, indent=2, sort_keys=True) + "\n")
    except (OSError, ValueError, KeyError, TypeError) as error:
        parser.exit(2, f"typed-feedback-profile: {error}\n")


if __name__ == "__main__":
    main()
