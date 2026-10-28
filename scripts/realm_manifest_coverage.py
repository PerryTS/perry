#!/usr/bin/env python3
"""Check that source declarations name every cell reached by a realm dump."""
import argparse
import heapq
import json
from pathlib import Path
import re


def property_key(prop):
    if "key" in prop:
        return prop["key"]
    display = prop["symbol"]
    if display.startswith("Symbol(Symbol."):
        return "@@" + display[14:-1]
    return "@runtime:" + display[7:-1]


def suffix(prop):
    key = property_key(prop)
    return "." + key if re.fullmatch(r"[A-Za-z_$][\w$]*", key) else "[" + json.dumps(key) + "]"


def cell_names(rows):
    """Prefer ordinary property paths to prototype paths; retain intrinsic seeds."""
    nodes = {row["path"]: row for row in rows}
    root = rows[0]
    names = {root["path"]: "%Global%"}
    distance = {root["path"]: 0}
    queue = [(0, root["path"])]
    root_refs = {}
    for prop in root["properties"]:
        for role, value in prop["descriptor"].items():
            if not isinstance(value, dict) or "ref" not in value:
                continue
            target = value["ref"]
            if role == "value":
                root_refs[property_key(prop)] = target
            if target not in distance or distance[target] > 1:
                distance[target] = 1
                names[target] = property_key(prop) + (".<" + role + ">" if role != "value" else "")
                heapq.heappush(queue, (1, target))
    for intrinsic in root.get("intrinsics", []):
        target = intrinsic["value"].get("ref")
        if target is not None and target not in distance:
            distance[target] = 100
            names[target] = intrinsic["name"]
            heapq.heappush(queue, (100, target))
    while queue:
        cost, path = heapq.heappop(queue)
        if cost != distance[path]:
            continue
        row = nodes[path]
        edges = []
        for prop in row["properties"]:
            for role, value in prop["descriptor"].items():
                if isinstance(value, dict) and "ref" in value:
                    edges.append((1, value["ref"], names[path] + suffix(prop) +
                                  (".<" + role + ">" if role != "value" else "")))
        if isinstance(row["prototype"], dict) and "ref" in row["prototype"]:
            edges.append((1000, row["prototype"]["ref"], names[path] + ".<prototype>"))
        for delta, target, name in edges:
            if target not in distance or cost + delta < distance[target]:
                distance[target] = cost + delta
                names[target] = name
                heapq.heappush(queue, (cost + delta, target))
    if len(names) != len(nodes):
        raise ValueError("realm dump contains unreachable cells")
    if "Uint8Array" in root_refs:
        ctor = nodes[root_refs["Uint8Array"]]
        if isinstance(ctor["prototype"], dict):
            names[ctor["prototype"]["ref"]] = "%TypedArray%"
        prototype = next(p["descriptor"]["value"]["ref"] for p in ctor["properties"]
                         if p.get("key") == "prototype")
        if isinstance(nodes[prototype]["prototype"], dict):
            names[nodes[prototype]["prototype"]["ref"]] = "%TypedArray%.prototype"
    if len(set(names.values())) != len(names):
        raise ValueError("realm cell names are ambiguous")
    return set(names.values())


def resolve_cell(name, nodes, roots):
    candidates = [root for root in roots if name == root or
                  name.startswith(root + ".") or name.startswith(root + "[")]
    if not candidates:
        return None
    root = max(candidates, key=len)
    value = roots[root]
    rest = name[len(root):]
    while True:
        if not isinstance(value, dict) or "ref" not in value:
            return None
        path = value["ref"]
        if not rest:
            return path
        row = nodes[path]
        if rest.startswith(".<prototype>"):
            value = row["prototype"]
            rest = rest[len(".<prototype>"):]
            continue
        if rest.startswith("["):
            key, length = json.JSONDecoder().raw_decode(rest[1:])
            if rest[1 + length:2 + length] != "]":
                return None
            rest = rest[2 + length:]
        else:
            match = re.match(r"\.([A-Za-z_$][\w$]*)", rest)
            if match is None:
                return None
            key = match[1]
            rest = rest[match.end():]
        descriptor = next((p["descriptor"] for p in row["properties"]
                           if property_key(p) == key), None)
        if descriptor is None:
            return None
        role = "value"
        for accessor in ("get", "set"):
            marker = ".<" + accessor + ">"
            if rest.startswith(marker):
                role = accessor
                rest = rest[len(marker):]
                break
        value = descriptor.get(role)


def check(rows, manifest):
    nodes = {row["path"]: row for row in rows}
    global_row = rows[0]
    roots = {"%Global%": {"ref": global_row["path"]}}
    for prop in global_row["properties"]:
        for role, value in prop["descriptor"].items():
            if isinstance(value, dict) and "ref" in value:
                roots[property_key(prop) + (".<" + role + ">" if role != "value" else "")] = value
    for intrinsic in global_row.get("intrinsics", []):
        roots[intrinsic["name"]] = intrinsic["value"]
    if "Uint8Array" in roots:
        ctor = nodes[roots["Uint8Array"]["ref"]]
        roots["%TypedArray%"] = ctor["prototype"]
        instance = next(p["descriptor"]["value"] for p in ctor["properties"] if p.get("key") == "prototype")
        roots["%TypedArray%.prototype"] = nodes[instance["ref"]]["prototype"]
    covered = {path for d in manifest["declarations"]
               if (path := resolve_cell(d["name"], nodes, roots)) is not None}
    missing_paths = set(nodes) - covered
    if missing_paths:
        # Names in a diagnostic may choose a different valid alias from the
        # source declarations. Coverage itself resolves paths, not aliases.
        observed = cell_names(rows)
        declared = {entry["name"] for entry in manifest["declarations"]}
        sample = ", ".join(sorted(observed - declared)[:25])
        raise ValueError("realm manifest omits " + str(len(missing_paths)) + " observed cells: " + sample)
    return len(nodes), len(manifest["declarations"])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("dump", type=Path)
    parser.add_argument("manifest", type=Path)
    args = parser.parse_args()
    rows = [json.loads(line) for line in args.dump.read_text().splitlines()]
    observed, declared = check(rows, json.loads(args.manifest.read_text()))
    print(f"realm manifest covers {observed} observed cells ({declared} declarations)")


if __name__ == "__main__":
    main()
