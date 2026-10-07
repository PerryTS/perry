#!/usr/bin/env python3
"""Select the workflow_call suites that belong in one category run."""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import re
import sys
from pathlib import Path
from typing import Any


ROOT = Path(__file__).resolve().parents[1]
CATALOG = ROOT / "scripts/actions_catalog.json"
PULL_REQUEST_TYPES = {"opened", "synchronize", "reopened"}


def load_catalog(path: Path = CATALOG) -> dict[str, Any]:
    data = json.loads(path.read_text())
    if data.get("schema_version") != 1 or not isinstance(data.get("categories"), dict):
        raise ValueError("actions catalog must use schema_version 1 and define categories")
    return data


def _glob_regex(pattern: str) -> re.Pattern[str]:
    """Translate the path-filter glob subset used by Actions without crossing `/` on `*`."""
    out = ["^"]
    i = 0
    while i < len(pattern):
        char = pattern[i]
        if char == "*":
            if i + 1 < len(pattern) and pattern[i + 1] == "*":
                i += 2
                if i < len(pattern) and pattern[i] == "/":
                    out.append("(?:.*/)?")
                    i += 1
                    continue
                out.append(".*")
                continue
            out.append("[^/]*")
        elif char == "?":
            out.append("[^/]")
        elif char == "[":
            end = pattern.find("]", i + 1)
            if end < 0:
                out.append(r"\[")
            else:
                cls = pattern[i + 1 : end]
                if cls.startswith("!"):
                    cls = "^" + cls[1:]
                out.append("[" + cls + "]")
                i = end
        else:
            out.append(re.escape(char))
        i += 1
    out.append("$")
    return re.compile("".join(out))


def path_matches(patterns: list[str], paths: list[str]) -> bool:
    """Apply ordered Actions include/exclude path patterns to a changed-file list."""
    included = False
    for pattern in patterns:
        negative = pattern.startswith("!")
        candidate = pattern[1:] if negative else pattern
        matcher = _glob_regex(candidate)
        matched = any(matcher.fullmatch(path) for path in paths)
        if negative and matched:
            included = False
        elif not negative and matched:
            included = True
    return included


def _event_rules(module: dict[str, Any], event_name: str) -> list[dict[str, Any]]:
    events = module.get("original_events") or {}
    if event_name not in events:
        return []
    raw = events[event_name]
    if raw is None:
        return [{}]
    return raw if isinstance(raw, list) else [raw]


def _matches_event(module: dict[str, Any], event_name: str, event: dict[str, Any], ref: str,
                   changed_paths: list[str] | None) -> bool:
    rules = _event_rules(module, event_name)
    if not rules:
        return False
    action = event.get("action")
    if event_name == "pull_request":
        base_ref = ((event.get("pull_request") or {}).get("base") or {}).get("ref")
        event_types = set()
        # The triggering action is distinct from the PR's current labels.
        for rule in rules:
            types = rule.get("types") if isinstance(rule, dict) else None
            event_types.update(types if types else PULL_REQUEST_TYPES)
        labels = ((event.get("pull_request") or {}).get("labels") or [])
        if any(isinstance(rule, dict) and rule.get("types") and "labeled" in rule["types"] for rule in rules):
            if action == "labeled":
                label_event = event.get("label") or {}
                if not label_event.get("name") or (module.get("pr_label") and label_event.get("name") != module["pr_label"]):
                    return False
                event_types = {"labeled"}
        if action not in event_types:
            return False
        if base_ref is None:
            base_ref = event.get("base_ref")
        for rule in rules:
            branches = rule.get("branches") if isinstance(rule, dict) else None
            if branches and not any(fnmatch.fnmatchcase(str(base_ref or ""), x) for x in branches):
                continue
            paths = rule.get("paths") if isinstance(rule, dict) else None
            if paths is not None and changed_paths is not None and not path_matches(paths, changed_paths):
                continue
            label = module.get("pr_label")
            if label and action != "labeled" and not any(item.get("name") == label for item in labels if isinstance(item, dict)):
                continue
            return True
        return False
    if event_name == "push":
        event_ref = event.get("ref") or ref
        for rule in rules:
            if not isinstance(rule, dict):
                rule = {}
            branches = rule.get("branches")
            tags = rule.get("tags")
            if event_ref.startswith("refs/heads/"):
                name = event_ref.removeprefix("refs/heads/")
                if tags or (branches and not any(fnmatch.fnmatchcase(name, x) for x in branches)):
                    continue
            elif event_ref.startswith("refs/tags/"):
                name = event_ref.removeprefix("refs/tags/")
                if branches or (tags and not any(fnmatch.fnmatchcase(name, x) for x in tags)):
                    continue
            else:
                continue
            paths = rule.get("paths")
            if paths is not None and changed_paths is not None and not path_matches(paths, changed_paths):
                continue
            return True
        return False
    if event_name == "release":
        for rule in rules:
            types = rule.get("types") if isinstance(rule, dict) else None
            if not types or action in types:
                return True
        return False
    if event_name == "workflow_run":
        upstream = event.get("workflow_run") or {}
        workflow = upstream.get("name")
        if upstream.get("event") == "schedule" and upstream.get("head_branch") not in (None, "main"):
            return False
        if upstream.get("event") == "pull_request":
            return False
        if upstream.get("head_branch") not in (None, "main") and not str(upstream.get("head_branch", "")).startswith("v"):
            return False
        return workflow in module.get("workflow_run_names", [])
    if event_name == "merge_group":
        return bool(module.get("merge_group"))
    if event_name == "schedule":
        cron = event.get("schedule")
        return any(isinstance(rule, dict) and cron == rule.get("cron") for rule in rules)
    return True


def _dispatch_inputs(module: dict[str, Any], category: dict[str, Any]) -> dict[str, Any]:
    """Map parent manual input values to the reusable workflow's typed inputs."""
    raw_inputs = module.get("inputs", {})
    result: dict[str, Any] = {}
    for key, spec in raw_inputs.items():
        mapping = module.get("parent_input_map", {}).get(key, key)
        value = category.get("dispatch_inputs", {}).get(mapping, spec).get("default")
        if spec.get("type") == "number" and isinstance(value, str):
            value = int(value)
        if spec.get("type") == "boolean" and isinstance(value, str):
            value = value.lower() == "true"
        result[key] = value
    return result


def _provided_inputs(module: dict[str, Any], category: dict[str, Any], event: dict[str, Any]) -> dict[str, Any]:
    """Preserve explicit parent dispatch values (including false and zero)."""
    parent_values = ((event.get("inputs") or {}) if isinstance(event.get("inputs"), dict) else {})
    result = _dispatch_inputs(module, category)
    for name in module.get("inputs", {}):
        mapping = module.get("parent_input_map", {}).get(name, name)
        if mapping in parent_values:
            result[name] = parent_values[mapping]
    return result


def select(category_key: str, event_name: str, event: dict[str, Any], ref: str = "refs/heads/main",
           changed_paths: list[str] | None = None, suite: str | None = None,
           catalog: dict[str, Any] | None = None) -> dict[str, Any]:
    catalog = catalog or load_catalog()
    categories = catalog["categories"]
    if category_key not in categories:
        raise ValueError(f"unknown workflow category: {category_key}")
    category = categories[category_key]
    modules = category["modules"]
    ids = [module["id"] for module in modules]
    if len(ids) != len(set(ids)):
        raise ValueError(f"duplicate module ID in category {category_key}")
    run_core = False
    if event_name == "workflow_dispatch":
        if category_key == "ci":
            requested = suite or "core"
            if requested not in categories[category_key]["dispatch_inputs"]["suite"]["options"]:
                raise ValueError(f"unknown CI suite selection: {requested}")
            run_core = requested in {"core", "all"}
            requested_ids = set(ids if requested == "all" else ([] if requested == "core" else [requested]))
        elif category_key == "maintenance":
            requested = suite or "validate"
            valid = categories[category_key]["dispatch_inputs"]["suite"]["options"]
            if requested == "all" or requested not in valid:
                raise ValueError(f"unknown Maintenance suite selection: {requested}")
            requested_ids = set([] if requested in {"validate", "delete-retired-runs"} else [requested])
        else:
            requested = suite or category.get("dispatch_inputs", {}).get("suite", {}).get("default", "all")
            valid = ["all", *ids]
            if requested not in valid:
                raise ValueError(f"unknown {category_key} suite selection: {requested}")
            requested_ids = set(m["id"] for m in modules if requested == "all" or m["id"] == requested)
            requested_ids = {mid for mid in requested_ids if not next(m for m in modules if m["id"] == mid).get("manual_only") or requested != "all" or mid == requested}
        selected = {module["id"] for module in modules if module["id"] in requested_ids}
        # Core CI is governed by its tier input; `all` adds auxiliaries too.
        if category_key == "ci" and requested == "all":
            selected.discard("core")
    elif event_name == "pull_request" and category_key == "ci":
        # A PR must always produce the protected pr-gate, even when only an
        # auxiliary workflow's paths changed.
        run_core = True
        selected = set()
        for module in modules:
            if module["id"] == "core" or not _matches_event(module, event_name, event, ref, changed_paths):
                continue
            selected.add(module["id"])
    else:
        selected = {module["id"] for module in modules
                    if event_name not in module.get("validation_only_events", [])
                    and not (module.get("manual_only") and event_name != "workflow_dispatch")
                    and _matches_event(module, event_name, event, ref, changed_paths)}
        if category_key == "ci":
            core = next(module for module in modules if module["id"] == "core")
            run_core = _matches_event(core, event_name, event, ref, changed_paths)
    if event_name == "schedule" and not selected and not run_core:
        raise ValueError(f"unrecognized {category_key} schedule: {event.get('schedule')!r}")
    if event_name == "schedule" and not any(
        isinstance(rule, dict) and event.get("schedule") == rule.get("cron")
        for module in modules for rule in _event_rules(module, "schedule")
    ):
        raise ValueError(f"unrecognized {category_key} schedule: {event.get('schedule')!r}")
    if category_key == "maintenance" and event_name == "workflow_dispatch" and requested != "validate" and ref != "refs/heads/main":
        raise ValueError(f"Maintenance suite {requested!r} may be dispatched only from main")
    plan = {module_id: module_id in selected for module_id in ids}
    if event_name == "workflow_dispatch":
        requested = suite
        if category_key == "ci": requested = requested or "core"
        elif category_key == "maintenance": requested = requested or "validate"
        else: requested = requested or category.get("dispatch_inputs", {}).get("suite", {}).get("default", "all")
        module_inputs = {
            module["id"]: _provided_inputs(module, category, event)
            for module in modules if module["id"] in selected
        }
        if requested == "all":
            module_inputs = {module["id"]: _dispatch_inputs(module, category)
                             for module in modules if module["id"] in selected}
    else:
        module_inputs = {}
    return {"plan": json.dumps(plan, separators=(",", ":")),
            "selection": ",".join(module_id for module_id in ids if module_id in selected),
            "module_inputs": json.dumps(module_inputs, separators=(",", ":")),
            "run_core": str(run_core).lower(),
            "validate": str(category_key == "maintenance" and
                            (event_name == "pull_request" or
                             (event_name == "workflow_dispatch" and (suite or "validate") == "validate"))).lower()}


def _load_paths(path: str | None) -> list[str] | None:
    if path is None:
        return None
    return Path(path).read_text().splitlines()


def _push_paths(event: dict[str, Any]) -> list[str] | None:
    """Return complete paths from the push payload; None means fail open.

    GitHub truncates large push payloads. Only apply per-file routing when the
    payload advertises and contains every commit; otherwise select all suites
    whose branch/tag rules match.
    """
    commits = event.get("commits")
    size = event.get("size")
    if not isinstance(commits, list) or not isinstance(size, int) or len(commits) != size:
        return None
    paths: set[str] = set()
    for commit in commits:
        if not isinstance(commit, dict):
            return None
        for key in ("added", "modified", "removed"):
            values = commit.get(key, [])
            if not isinstance(values, list) or any(not isinstance(item, str) for item in values):
                return None
            paths.update(values)
    return sorted(paths)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--category", required=True)
    parser.add_argument("--event", required=True)
    parser.add_argument("--event-path", required=True)
    parser.add_argument("--ref", required=True)
    parser.add_argument("--changed-files")
    parser.add_argument("--suite")
    args = parser.parse_args(argv)
    try:
        event = json.loads(Path(args.event_path).read_text())
        if not isinstance(event, dict):
            raise ValueError("GitHub event payload must be a JSON object")
        changed_paths = _load_paths(args.changed_files)
        if args.event == "push" and changed_paths is None:
            changed_paths = _push_paths(event)
        result = select(args.category, args.event, event, args.ref,
                        changed_paths, args.suite)
    except (OSError, json.JSONDecodeError, ValueError) as exc:
        print(f"::error::{exc}", file=sys.stderr)
        return 2
    if "GITHUB_OUTPUT" in os.environ:
        with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
            for key, value in result.items():
                output.write(f"{key}={value}\n")
    else:
        print(json.dumps(result, indent=2))
    if "GITHUB_STEP_SUMMARY" in os.environ:
        with open(os.environ["GITHUB_STEP_SUMMARY"], "a", encoding="utf-8") as summary:
            summary.write(f"## {args.category} workflow routing\n\n")
            summary.write(f"Event: `{args.event}`  Ref: `{args.ref}`  Schedule: `{event.get('schedule', '—')}`\n\n")
            summary.write(f"Selected suites: `{result['selection'] or 'none'}`\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
