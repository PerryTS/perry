#!/usr/bin/env python3
"""Audit Perry's Cargo workspace shape and architectural policy.

The Cargo workspace is the source of truth for packages and dependency edges;
`workspace-architecture.json` records the human decision for every package.
This script joins both views so CI can reject accidental members, unclassified
crates, expanded default builds, missing workspace lints, and new production
dependencies from native bindings into `perry-runtime`.

Usage:
  python3 scripts/workspace_architecture.py --check --print-summary
  python3 scripts/workspace_architecture.py --markdown
  python3 scripts/workspace_architecture.py --json
  python3 scripts/workspace_architecture.py --self-test
"""

import argparse
import glob
import json
import re
import subprocess
import sys
import tempfile
import unittest
from collections import defaultdict
from pathlib import Path
from unittest import mock


ROOT = Path(__file__).resolve().parents[1]
ROOT_MANIFEST = ROOT / "Cargo.toml"
POLICY_PATH = ROOT / "workspace-architecture.json"

ALLOWED_CATEGORIES = {
    "artifact-wrapper",
    "binding",
    "codegen-adapter",
    "compiler-core",
    "fixture",
    "platform-adapter",
    "platform-core",
    "product",
    "runtime-core",
    "test-support",
    "tool",
}
ALLOWED_DECISIONS = {"keep", "merge", "externalize", "remove", "review"}
SUPPORTED_SCHEMA_VERSION = 2
BASELINE_FIELDS = {"default_dependency_closure", "perry_dependency_closure"}


def load_metadata():
    raw = subprocess.check_output(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=str(ROOT),
    )
    return json.loads(raw)


def load_policy():
    with POLICY_PATH.open(encoding="utf-8") as handle:
        policy = json.load(handle)
    validate_policy_schema(policy)
    return policy


def validate_policy_schema(policy):
    version = policy.get("schema_version")
    if version != SUPPORTED_SCHEMA_VERSION:
        raise ValueError(
            "unsupported workspace architecture schema version: {!r}; "
            "expected {}. Migrate the policy by removing baseline.workspace_members "
            "and baseline.decision_counts; exact dependency closures remain "
            "required structural checks.".format(version, SUPPORTED_SCHEMA_VERSION)
        )
    baseline = policy.get("baseline", {})
    if not isinstance(baseline, dict):
        raise ValueError("workspace architecture baseline must be an object")
    stale = set(baseline) - BASELINE_FIELDS
    if stale:
        raise ValueError(
            "unsupported workspace architecture baseline field(s): {}; "
            "remove derived aggregates and retain only exact dependency closures".format(
                ", ".join(sorted(stale))
            )
        )


def excluded_members(policy, scope):
    """Return the centrally maintained Linux exclusion set for a CI scope."""
    ci = policy.get("ci", {})
    excluded = set(ci.get("linux_host_excluded_members", []))
    if scope == "linux-test":
        excluded.update(ci.get("linux_test_extra_excluded_members", []))
    elif scope != "host-compatible":
        raise ValueError("unknown package scope: {}".format(scope))
    return excluded


def package_scope(metadata, policy, scope):
    packages = workspace_packages(metadata)
    if scope == "product":
        return ["perry"]
    return sorted(set(packages) - excluded_members(policy, scope))


def workspace_packages(metadata):
    workspace_ids = set(metadata["workspace_members"])
    return {
        package["name"]: package
        for package in metadata["packages"]
        if package["id"] in workspace_ids
    }


def _workspace_section(text):
    match = re.search(r"(?ms)^\[workspace\]\s*(.*?)(?=^\[[^\n]+\]|\Z)", text)
    if not match:
        raise ValueError("Cargo.toml has no [workspace] section")
    return match.group(1)


def _quoted_list(section, key):
    match = re.search(
        r"(?ms)^" + re.escape(key) + r"\s*=\s*\[(.*?)\]", section
    )
    if not match:
        raise ValueError("[workspace] has no {} list".format(key))
    return re.findall(r'"([^"]+)"', match.group(1))


def declared_workspace_paths(manifest_text):
    section = _workspace_section(manifest_text)
    declared = set()
    for pattern in _quoted_list(section, "members"):
        if any(character in pattern for character in "*?["):
            matches = glob.glob(str(ROOT / pattern))
        else:
            matches = [str(ROOT / pattern)]
        for match in matches:
            path = Path(match)
            crate_dir = path.parent if path.name == "Cargo.toml" else path
            declared.add(crate_dir.resolve())
    return declared


def declared_default_paths(manifest_text):
    section = _workspace_section(manifest_text)
    return {
        (ROOT / path).resolve()
        for path in _quoted_list(section, "default-members")
    }


def package_path(package):
    return Path(package["manifest_path"]).resolve().parent


def production_internal_dependencies(package, packages):
    names = set(packages)
    return {
        dep["name"]
        for dep in package.get("dependencies", [])
        if dep["name"] in names and dep.get("kind") != "dev"
    }


def dependency_closure(packages, seeds):
    graph = {
        name: production_internal_dependencies(package, packages)
        for name, package in packages.items()
    }
    reached = set(seeds)
    pending = list(seeds)
    while pending:
        current = pending.pop()
        for dependency in graph.get(current, ()):
            if dependency not in reached:
                reached.add(dependency)
                pending.append(dependency)
    return reached


def rust_metrics(package):
    source = package_path(package) / "src"
    files = sorted(source.rglob("*.rs")) if source.exists() else []
    lines = 0
    for path in files:
        try:
            with path.open(encoding="utf-8", errors="ignore") as handle:
                lines += sum(1 for _ in handle)
        except OSError:
            pass
    return {"rust_files": len(files), "rust_lines": lines}


def has_workspace_lints(package):
    text = Path(package["manifest_path"]).read_text(encoding="utf-8")
    return bool(
        re.search(
            r"(?ms)^\[lints\]\s*(.*?)(?=^\[[^\n]+\]|\Z)", text
        )
        and re.search(
            r"(?m)^\s*workspace\s*=\s*true\s*$",
            re.search(
                r"(?ms)^\[lints\]\s*(.*?)(?=^\[[^\n]+\]|\Z)", text
            ).group(1),
        )
    )


def build_inventory(metadata, policy):
    packages = workspace_packages(metadata)
    default_ids = set(metadata["workspace_default_members"])
    default_names = {
        package["name"]
        for package in packages.values()
        if package["id"] in default_ids
    }
    reverse = defaultdict(set)
    for name, package in packages.items():
        for dependency in production_internal_dependencies(package, packages):
            reverse[dependency].add(name)

    rows = []
    decisions = policy.get("crates", {})
    for name, package in sorted(packages.items()):
        metrics = rust_metrics(package)
        classification = decisions.get(name, {})
        rows.append(
            {
                "name": name,
                "path": str(package_path(package).relative_to(ROOT)),
                "category": classification.get("category", "unclassified"),
                "decision": classification.get("decision", "unclassified"),
                "default_member": name in default_names,
                "production_dependencies": sorted(
                    production_internal_dependencies(package, packages)
                ),
                "reverse_dependencies": sorted(reverse[name]),
                "rust_files": metrics["rust_files"],
                "rust_lines": metrics["rust_lines"],
                "workspace_lints": has_workspace_lints(package),
            }
        )
    return rows


def audit(metadata, policy, manifest_text):
    errors = []
    packages = workspace_packages(metadata)
    effective_paths = {package_path(package) for package in packages.values()}
    declared_paths = declared_workspace_paths(manifest_text)

    implicit = effective_paths - declared_paths
    stale = declared_paths - effective_paths
    if implicit:
        errors.append(
            "implicit workspace members: "
            + ", ".join(str(path.relative_to(ROOT)) for path in sorted(implicit))
        )
    if stale:
        errors.append(
            "declared workspace paths not present in cargo metadata: "
            + ", ".join(str(path.relative_to(ROOT)) for path in sorted(stale))
        )

    policy_crates = set(policy.get("crates", {}))
    package_names = set(packages)
    missing = package_names - policy_crates
    removed = policy_crates - package_names
    if missing:
        errors.append("unclassified workspace crates: " + ", ".join(sorted(missing)))
    if removed:
        errors.append("policy entries without workspace crates: " + ", ".join(sorted(removed)))

    for name, classification in sorted(policy.get("crates", {}).items()):
        category = classification.get("category")
        decision = classification.get("decision")
        if category not in ALLOWED_CATEGORIES:
            errors.append("{} has invalid category {!r}".format(name, category))
        if decision not in ALLOWED_DECISIONS:
            errors.append("{} has invalid decision {!r}".format(name, decision))

    default_ids = set(metadata["workspace_default_members"])
    actual_defaults = sorted(
        package["name"]
        for package in packages.values()
        if package["id"] in default_ids
    )
    expected_defaults = sorted(policy.get("expected_default_members", []))
    if actual_defaults != expected_defaults:
        errors.append(
            "default members differ: expected {}, got {}".format(
                expected_defaults, actual_defaults
            )
        )

    manifest_defaults = declared_default_paths(manifest_text)
    metadata_defaults = {
        package_path(package)
        for package in packages.values()
        if package["id"] in default_ids
    }
    if manifest_defaults != metadata_defaults:
        errors.append("Cargo default-members do not match cargo metadata")

    runtime_dependents = sorted(
        name
        for name, package in packages.items()
        if name.startswith("perry-ext-")
        and "perry-runtime" in production_internal_dependencies(package, packages)
    )
    allowed_runtime_dependents = sorted(
        policy.get("allowed_binding_runtime_dependencies", [])
    )
    if runtime_dependents != allowed_runtime_dependents:
        errors.append(
            "production binding -> runtime edges differ: expected {}, got {}".format(
                allowed_runtime_dependents, runtime_dependents
            )
        )

    missing_lints = sorted(
        name for name, package in packages.items() if not has_workspace_lints(package)
    )
    if missing_lints:
        errors.append(
            "crates missing [lints] workspace = true: "
            + ", ".join(missing_lints)
        )

    ci = policy.get("ci", {})
    host_excluded = set(ci.get("linux_host_excluded_members", []))
    test_extra_excluded = set(ci.get("linux_test_extra_excluded_members", []))
    unknown_exclusions = (host_excluded | test_extra_excluded) - package_names
    if unknown_exclusions:
        errors.append(
            "CI exclusions without workspace crates: "
            + ", ".join(sorted(unknown_exclusions))
        )
    non_platform_host_exclusions = sorted(
        name
        for name in host_excluded
        if policy.get("crates", {}).get(name, {}).get("category")
        != "platform-adapter"
    )
    if non_platform_host_exclusions:
        errors.append(
            "Linux host exclusions must be platform adapters: "
            + ", ".join(non_platform_host_exclusions)
        )

    baseline = policy.get("baseline", {})
    cli_closure = sorted(dependency_closure(packages, {"perry"}))
    default_closure = sorted(dependency_closure(packages, set(actual_defaults)))
    expected_baseline = {
        "default_dependency_closure": default_closure,
        "perry_dependency_closure": cli_closure,
    }
    if baseline != expected_baseline:
        errors.append(
            "architecture baseline differs; review the structural change and "
            "refresh workspace-architecture.json"
        )

    return errors


def summary(metadata, policy):
    packages = workspace_packages(metadata)
    default_ids = set(metadata["workspace_default_members"])
    default_names = {
        package["name"]
        for package in packages.values()
        if package["id"] in default_ids
    }
    cli_closure = dependency_closure(packages, {"perry"})
    default_closure = dependency_closure(packages, default_names)
    decisions = defaultdict(int)
    categories = defaultdict(int)
    for classification in policy.get("crates", {}).values():
        decisions[classification["decision"]] += 1
        categories[classification["category"]] += 1
    return {
        "workspace_members": len(packages),
        "default_members": len(default_names),
        "default_dependency_closure": len(default_closure),
        "perry_dependency_closure": len(cli_closure),
        "categories": dict(sorted(categories.items())),
        "decisions": dict(sorted(decisions.items())),
    }


def print_summary(data):
    print("Workspace architecture summary")
    print("  workspace members:          {}".format(data["workspace_members"]))
    print("  default members:            {}".format(data["default_members"]))
    print("  default dependency closure: {}".format(data["default_dependency_closure"]))
    print("  perry dependency closure:   {}".format(data["perry_dependency_closure"]))
    print("  decisions:                  {}".format(
        ", ".join("{}={}".format(k, v) for k, v in data["decisions"].items())
    ))


def print_markdown(rows):
    print(
        "| Crate | Path | Category | Decision | Default | Rust LOC | "
        "Internal dependencies | Internal consumers | Workspace lints |"
    )
    print("|---|---|---|---|---:|---:|---|---|---:|")
    for row in rows:
        dependencies = ", ".join(
            "`{}`".format(x) for x in row["production_dependencies"]
        ) or "—"
        consumers = ", ".join(
            "`{}`".format(x) for x in row["reverse_dependencies"]
        ) or "—"
        print(
            "| `{}` | `{}` | {} | {} | {} | {} | {} | {} | {} |".format(
                row["name"],
                row["path"],
                row["category"],
                row["decision"],
                "yes" if row["default_member"] else "no",
                row["rust_lines"],
                dependencies,
                consumers,
                "yes" if row["workspace_lints"] else "no",
            )
        )


class HelpersTest(unittest.TestCase):
    def test_workspace_lists_are_scoped_to_workspace_section(self):
        text = '''
[workspace]
members = ["crates/a", "crates/b"]
default-members = ["crates/a"]

[workspace.dependencies]
something = "1"
'''
        section = _workspace_section(text)
        self.assertEqual(_quoted_list(section, "members"), ["crates/a", "crates/b"])
        self.assertEqual(_quoted_list(section, "default-members"), ["crates/a"])

    def test_dependency_closure_ignores_dev_dependencies(self):
        packages = {
            "a": {"dependencies": [{"name": "b", "kind": None}]},
            "b": {"dependencies": [{"name": "c", "kind": "dev"}]},
            "c": {"dependencies": []},
        }
        self.assertEqual(dependency_closure(packages, {"a"}), {"a", "b"})

    def test_ci_scopes_share_the_host_exclusion_source(self):
        policy = {
            "ci": {
                "linux_host_excluded_members": ["ui"],
                "linux_test_extra_excluded_members": ["fixture"],
            }
        }
        self.assertEqual(excluded_members(policy, "host-compatible"), {"ui"})
        self.assertEqual(excluded_members(policy, "linux-test"), {"ui", "fixture"})

    def test_unknown_ci_scope_is_rejected(self):
        with self.assertRaises(ValueError):
            excluded_members({}, "typo")

    def test_policy_schema_version_is_required(self):
        with self.assertRaisesRegex(ValueError, "schema version"):
            validate_policy_schema({})

    def test_unknown_policy_schema_version_is_rejected(self):
        with self.assertRaisesRegex(ValueError, "schema version"):
            validate_policy_schema({"schema_version": 3})

    def test_supported_policy_schema_version_is_accepted(self):
        validate_policy_schema(
            {
                "schema_version": SUPPORTED_SCHEMA_VERSION,
                "baseline": {
                    "default_dependency_closure": [],
                    "perry_dependency_closure": [],
                },
            }
        )

    def test_schema_one_has_actionable_migration_error(self):
        with self.assertRaisesRegex(ValueError, "removing baseline.workspace_members"):
            validate_policy_schema({"schema_version": 1})

    def test_aggregate_baseline_fields_are_rejected_in_schema_two(self):
        closures = {
            "default_dependency_closure": [],
            "perry_dependency_closure": [],
        }
        for field, value in (
            ("workspace_members", 4),
            ("decision_counts", {"keep": 4}),
            ("unexpected", True),
        ):
            with self.subTest(field=field):
                with self.assertRaisesRegex(ValueError, field):
                    validate_policy_schema(
                        {
                            "schema_version": 2,
                            "baseline": {**closures, field: value},
                        }
                    )

    def test_checked_in_policy_uses_only_exact_closure_baselines(self):
        with POLICY_PATH.open(encoding="utf-8") as handle:
            policy = json.load(handle)
        validate_policy_schema(policy)
        self.assertEqual(set(policy["baseline"]), BASELINE_FIELDS)

    @staticmethod
    def _write_workspace(root, names, default_names=("perry",)):
        member_paths = ["crates/{}".format(name) for name in names]
        default_paths = ["crates/{}".format(name) for name in default_names]
        lines = ["[workspace]", "members = ["]
        lines.extend('    "{}",'.format(path) for path in member_paths)
        lines.extend(["]", "default-members = ["])
        lines.extend('    "{}",'.format(path) for path in default_paths)
        lines.append("]")
        text = "\n".join(lines) + "\n"
        (root / "Cargo.toml").write_text(text, encoding="utf-8")
        return text

    @classmethod
    def _audit_fixture(cls, root):
        names = ["perry", "perry-runtime", "leaf-a", "leaf-b"]
        dependencies = {"perry": ["perry-runtime"]}
        packages = []
        for name in names:
            crate = root / "crates" / name
            (crate / "src").mkdir(parents=True)
            manifest_path = crate / "Cargo.toml"
            manifest_path.write_text(
                "[package]\nname = \"{}\"\nversion = \"0.1.0\"\n\n"
                "[lints]\nworkspace = true\n".format(name),
                encoding="utf-8",
            )
            packages.append(
                {
                    "id": name,
                    "name": name,
                    "manifest_path": str(manifest_path),
                    "dependencies": [
                        {"name": dependency, "kind": None}
                        for dependency in dependencies.get(name, [])
                    ],
                }
            )
        manifest = cls._write_workspace(root, names)
        metadata = {
            "packages": packages,
            "workspace_members": names.copy(),
            "workspace_default_members": ["perry"],
        }
        policy = {
            "schema_version": 2,
            "expected_default_members": ["perry"],
            "allowed_binding_runtime_dependencies": [],
            "ci": {
                "linux_host_excluded_members": [],
                "linux_test_extra_excluded_members": [],
            },
            "baseline": {
                "default_dependency_closure": ["perry", "perry-runtime"],
                "perry_dependency_closure": ["perry", "perry-runtime"],
            },
            "crates": {
                "perry": {"category": "product", "decision": "keep"},
                "perry-runtime": {
                    "category": "runtime-core",
                    "decision": "keep",
                },
                "leaf-a": {"category": "tool", "decision": "keep"},
                "leaf-b": {"category": "tool", "decision": "keep"},
            },
        }
        return metadata, policy, manifest

    @staticmethod
    def _drop_fixture_member(root, metadata, policy, manifest_text, name):
        metadata["packages"] = [
            package for package in metadata["packages"] if package["name"] != name
        ]
        metadata["workspace_members"].remove(name)
        policy["crates"].pop(name)
        names = [package["name"] for package in metadata["packages"]]
        return HelpersTest._write_workspace(root, names), metadata, policy

    def _audit_fixture_at(self, root, metadata, policy, manifest):
        # Production ROOT and package_path both resolve symlinks. Mirror that
        # for macOS temporary directories (/var -> /private/var) as well.
        with mock.patch(__name__ + ".ROOT", root.resolve()):
            validate_policy_schema(policy)
            return audit(metadata, policy, manifest)

    def test_independent_and_composed_leaf_removals_need_no_aggregate_edits(self):
        for removals in (("leaf-a",), ("leaf-b",), ("leaf-a", "leaf-b"),
                         ("leaf-b", "leaf-a")):
            with self.subTest(removals=removals), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                metadata, policy, manifest = self._audit_fixture(root)
                for name in removals:
                    manifest, metadata, policy = self._drop_fixture_member(
                        root, metadata, policy, manifest, name
                    )
                self.assertEqual(self._audit_fixture_at(root, metadata, policy, manifest), [])

    def test_new_member_without_classification_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, _ = self._audit_fixture(root)
            crate = root / "crates" / "new-tool"
            crate.mkdir(parents=True)
            manifest_path = crate / "Cargo.toml"
            manifest_path.write_text("[package]\nname = \"new-tool\"\n", encoding="utf-8")
            metadata["packages"].append(
                {"id": "new-tool", "name": "new-tool", "manifest_path": str(manifest_path), "dependencies": []}
            )
            metadata["workspace_members"].append("new-tool")
            manifest = self._write_workspace(
                root, metadata["workspace_members"]
            )
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("unclassified workspace crates: new-tool" in e for e in errors))

    def test_stale_policy_entry_fails(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, manifest = self._audit_fixture(root)
            policy["crates"]["removed-crate"] = {"category": "tool", "decision": "keep"}
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("policy entries without workspace crates" in e for e in errors))

    def test_implicit_and_stale_declared_members_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, _ = self._audit_fixture(root)
            manifest = self._write_workspace(root, ["perry", "perry-runtime", "leaf-b"])
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("implicit workspace members" in e for e in errors))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, _ = self._audit_fixture(root)
            manifest = self._write_workspace(
                root,
                ["perry", "perry-runtime", "leaf-a", "leaf-b", "stale-path"],
            )
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("declared workspace paths not present" in e for e in errors))

    def test_expanded_default_members_and_dependency_closure_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, _ = self._audit_fixture(root)
            manifest = self._write_workspace(
                root,
                ["perry", "perry-runtime", "leaf-a", "leaf-b"],
                default_names=("perry", "leaf-a"),
            )
            metadata["workspace_default_members"].append("leaf-a")
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("default members differ" in e for e in errors))
            self.assertTrue(any("baseline differs" in e for e in errors))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, manifest = self._audit_fixture(root)
            perry = next(p for p in metadata["packages"] if p["name"] == "perry")
            perry["dependencies"].append({"name": "leaf-a", "kind": None})
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("baseline differs" in e for e in errors))

    def test_each_exact_dependency_closure_remains_independently_checked(self):
        for field in ("default_dependency_closure", "perry_dependency_closure"):
            with self.subTest(field=field), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                metadata, policy, manifest = self._audit_fixture(root)
                policy["baseline"][field] = ["deliberately-wrong"]
                errors = self._audit_fixture_at(root, metadata, policy, manifest)
                self.assertTrue(any("baseline differs" in e for e in errors))

    def test_binding_runtime_edge_and_allowlist_change_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, _ = self._audit_fixture(root)
            name = "perry-ext-example"
            crate = root / "crates" / name
            crate.mkdir(parents=True)
            manifest_path = crate / "Cargo.toml"
            manifest_path.write_text("[package]\nname = \"{}\"\n".format(name), encoding="utf-8")
            metadata["packages"].append(
                {
                    "id": name,
                    "name": name,
                    "manifest_path": str(manifest_path),
                    "dependencies": [{"name": "perry-runtime", "kind": None}],
                }
            )
            metadata["workspace_members"].append(name)
            policy["crates"][name] = {"category": "binding", "decision": "keep"}
            manifest = self._write_workspace(root, metadata["workspace_members"])
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("production binding -> runtime edges differ" in e for e in errors))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, manifest = self._audit_fixture(root)
            policy["allowed_binding_runtime_dependencies"] = ["perry-ext-example"]
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("production binding -> runtime edges differ" in e for e in errors))

    def test_missing_workspace_lints_and_bad_ci_exclusions_fail(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, _ = self._audit_fixture(root)
            path = Path(next(p["manifest_path"] for p in metadata["packages"] if p["name"] == "leaf-a"))
            path.write_text("[package]\nname = \"leaf-a\"\n", encoding="utf-8")
            manifest = (root / "Cargo.toml").read_text(encoding="utf-8")
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("crates missing [lints]" in e for e in errors))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, manifest = self._audit_fixture(root)
            policy["ci"]["linux_host_excluded_members"] = ["perry"]
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("must be platform adapters" in e for e in errors))
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            metadata, policy, manifest = self._audit_fixture(root)
            policy["ci"]["linux_test_extra_excluded_members"] = ["unknown"]
            errors = self._audit_fixture_at(root, metadata, policy, manifest)
            self.assertTrue(any("CI exclusions without workspace crates" in e for e in errors))


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--markdown", action="store_true")
    parser.add_argument("--print-summary", action="store_true")
    parser.add_argument(
        "--print-package-scope",
        choices=("product", "host-compatible", "linux-test"),
    )
    parser.add_argument(
        "--print-excluded-scope",
        choices=("host-compatible", "linux-test"),
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        suite = unittest.defaultTestLoader.loadTestsFromTestCase(HelpersTest)
        return 0 if unittest.TextTestRunner(verbosity=2).run(suite).wasSuccessful() else 1

    metadata = load_metadata()
    policy = load_policy()
    manifest_text = ROOT_MANIFEST.read_text(encoding="utf-8")
    if args.print_package_scope:
        print("\n".join(package_scope(metadata, policy, args.print_package_scope)))
        return 0
    if args.print_excluded_scope:
        print("\n".join(sorted(excluded_members(policy, args.print_excluded_scope))))
        return 0

    rows = build_inventory(metadata, policy)
    data = summary(metadata, policy)

    if args.json:
        print(json.dumps({"summary": data, "crates": rows}, indent=2, sort_keys=True))
    elif args.markdown:
        print_markdown(rows)
    elif args.print_summary or not args.check:
        print_summary(data)

    if args.check:
        errors = audit(metadata, policy, manifest_text)
        if errors:
            for error in errors:
                print("workspace architecture error: {}".format(error), file=sys.stderr)
            return 1
        print("Workspace architecture policy: OK")
    return 0


if __name__ == "__main__":
    sys.exit(main())
