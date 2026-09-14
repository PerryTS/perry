#!/usr/bin/env python3
"""Read-only Rust dependency census; no builds, manifest edits, or lock updates.

Run from the repository root with Python 3.11+ and Cargo. Metadata describes
the cross-target lock graph, including optional dependencies; it is NOT a
measurement of linked code. Use cargo tree for selected build configurations.
Source references are lexical review aids, not proof that an edge is unused.
Only files within each package are scanned; shared-source #[path] modules,
macro expansion and feature-only dependencies need separate review.
"""

import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import re
import subprocess
import tomllib


def command(*args):
    return subprocess.check_output(args, text=True, encoding="utf-8")


def read_json(path):
    raw = path.read_bytes()
    encoding = "utf-16" if raw.startswith(b"\xff\xfe") else "utf-8-sig"
    return json.loads(raw.decode(encoding))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--metadata", type=Path, help="Reuse cargo metadata JSON")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path(command("git", "rev-parse", "--show-toplevel").strip())
    metadata = (read_json(args.metadata) if args.metadata else
                json.loads(command("cargo", "metadata", "--locked", "--format-version", "1")))
    packages = {p["id"]: p for p in metadata["packages"]}
    members = set(metadata["workspace_members"])
    nodes = {n["id"]: n for n in metadata["resolve"]["nodes"]}
    external = set(packages) - members
    lib_names = {"md-5": "md5", "cairo-rs": "cairo", "kamadak-exif": "exif",
                 "rustls-webpki": "webpki"}

    def label(pid):
        p = packages[pid]
        suffix = " [vendored]" if pid in external and p["source"] is None else ""
        return f"{p['name']}@{p['version']}{suffix}"

    def relative(path):
        return Path(path).relative_to(root).as_posix()

    def reachable(roots, skip_direct_name=None):
        seen = set()
        queue = list(roots)
        while queue:
            pid = queue.pop()
            if pid in seen:
                continue
            seen.add(pid)
            for dep in nodes[pid]["deps"]:
                target = dep["pkg"]
                if pid in members and packages[target]["name"] == skip_direct_name:
                    continue
                queue.append(target)
        return seen

    baseline = reachable(members)
    direct = defaultdict(list)
    token = re.compile(r"\b([A-Za-z_]\w*)\s*(?:::|!|[,;}])")
    for pid in sorted(members, key=label):
        p = packages[pid]
        package_root = Path(p["manifest_path"]).parent
        deps = [d for d in p["dependencies"] if d.get("source") or
                (d.get("path") and "third_party" in Path(d["path"]).parts)]
        aliases = {(d.get("rename") or lib_names.get(d["name"], d["name"])).replace("-", "_")
                   for d in deps}
        references = defaultdict(list)
        files = []
        for folder in ("src", "tests", "examples", "benches"):
            files.extend((package_root / folder).rglob("*.rs"))
        if (package_root / "build.rs").exists():
            files.append(package_root / "build.rs")
        for path in sorted(set(files)):
            for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
                if line.lstrip().startswith("//"):
                    continue
                for name in set(token.findall(line)) & aliases:
                    references[name].append(f"{relative(path)}:{number}")
        for dep in deps:
            alias = (dep.get("rename") or lib_names.get(dep["name"], dep["name"])).replace("-", "_")
            resolved = [d["pkg"] for d in nodes[pid]["deps"]
                        if d["name"] == alias and packages[d["pkg"]]["name"] == dep["name"]]
            # Cargo node names use actual Rust lib names, including renamed edges.
            if not resolved:
                resolved = [d["pkg"] for d in nodes[pid]["deps"]
                            if packages[d["pkg"]]["name"] == dep["name"]]
            feature_activators = [f for f, values in p["features"].items()
                                  if any(v in {"dep:" + (dep.get("rename") or dep["name"]),
                                               dep.get("rename") or dep["name"]} for v in values)]
            direct[dep["name"]].append({
                "owner": p["name"], "manifest": relative(p["manifest_path"]),
                "alias": alias, "kind": dep["kind"] or "normal", "target": dep["target"],
                "optional": dep["optional"], "requirement": dep["req"],
                "default_features": dep["uses_default_features"], "features": dep["features"],
                "direct_feature_activators": feature_activators,
                "resolved": sorted({label(x) for x in resolved}),
                "reference_lines": len(references[alias]), "references": references[alias][:6],
            })
    parents = defaultdict(list)
    for pid, node in nodes.items():
        for dep in node["deps"]:
            parents[dep["pkg"]].append(label(pid))
    records = []
    for pid in sorted(external, key=label):
        p = packages[pid]
        records.append({
            "package": label(pid), "source": p["source"] or relative(p["manifest_path"]),
            "name_is_direct_dependency": p["name"] in direct,
            "lock_features": nodes[pid]["features"],
            "parents": sorted(parents[pid]),
            "dependencies": sorted(label(d["pkg"]) for d in nodes[pid]["deps"]),
        })
    direct_records = []
    for name, edges in sorted(direct.items()):
        ids = {pid for pid in external if packages[pid]["name"] == name}
        lost = (baseline - reachable(members, name)) & external
        direct_records.append({
            "name": name, "versions": sorted({packages[pid]["version"] for pid in ids}),
            "edges": edges, "lock_closure_packages": len(reachable(ids) & external),
            "lock_packages_lost_if_all_workspace_edges_removed": sorted(label(pid) for pid in lost),
        })
    owned_manifests = {relative(packages[pid]["manifest_path"]) for pid in members}
    other_manifests = []
    manifests = command("git", "ls-files", "--", "*Cargo.toml", "Cargo.toml").splitlines()
    for manifest in sorted(set(manifests) - owned_manifests - {"Cargo.toml"}):
        parsed = tomllib.loads((root / manifest).read_text(encoding="utf-8"))
        sections = [(k, None, parsed.get(k, {})) for k in
                    ("dependencies", "dev-dependencies", "build-dependencies")]
        for target, tables in parsed.get("target", {}).items():
            sections.extend((k, target, tables.get(k, {})) for k in
                            ("dependencies", "dev-dependencies", "build-dependencies"))
        declarations = []
        for kind, target, table in sections:
            for alias, value in table.items():
                value = {"version": value} if isinstance(value, str) else value
                if not value.get("path") and not value.get("workspace"):
                    declarations.append({"name": value.get("package", alias),
                                         "kind": kind, "target": target, **value})
        other_manifests.append({"manifest": manifest,
                                "vendored": manifest.startswith("third_party/"),
                                "external_declarations": declarations})
    counts = Counter(p["name"] for pid, p in packages.items() if pid in external and p["source"])
    output = {
        "commit": command("git", "rev-parse", "HEAD").strip(),
        "cargo": command("cargo", "--version").strip(),
        "semantics": "Cross-target lock graph, including optional and dev/build dependencies; not linked size. Lexical references need manual review. Marginal counts freeze third-party edges/features and remove all workspace edges to one package name.",
        "summary": {"workspace_members": len(members), "direct_external_names": len(direct),
                    "registry_package_versions": sum(counts.values()),
                    "registry_package_names": len(counts),
                    "vendored_package_versions": sum(packages[p]["source"] is None for p in external),
                    "registry_names_with_multiple_versions": sum(n > 1 for n in counts.values())},
        "direct": direct_records, "external_lock_packages": records,
        "other_manifests": other_manifests,
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    # One record per line keeps the complete census reviewable without huge indentation.
    with args.output.open("w", encoding="utf-8", newline="\n") as f:
        f.write("{\n")
        for index, (key, value) in enumerate(output.items()):
            if index:
                f.write(",\n")
            f.write(json.dumps(key) + ": ")
            if isinstance(value, list):
                f.write("[\n" + ",\n".join(json.dumps(v, ensure_ascii=False) for v in value) + "\n]")
            else:
                f.write(json.dumps(value, ensure_ascii=False))
        f.write("\n}\n")
    print(json.dumps(output["summary"], indent=2))
    print(f"Wrote {args.output}")


if __name__ == "__main__":
    main()
