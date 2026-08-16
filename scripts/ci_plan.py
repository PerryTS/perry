#!/usr/bin/env python3
"""Decide which CI jobs a run of `.github/workflows/test.yml` executes.

WHY THIS EXISTS
---------------
`test.yml` used to answer "does job X run in this event?" with a different
`if:` on each of 25 jobs — event names, labels, refs and a dispatch input, all
spelled slightly differently. Two required contexts (`parity`, `compile-smoke`)
never ran on a pull request at all, a third (`conformance-smoke-complete`) had
been red on `main` for days, and every PR fanned out to 48 jobs against an
org-wide cap of 20 concurrent runners. Measured 2026-08-16: 0 of 66 PR runs of
`Tests` reached a conclusion; the last 12 merges all bypassed branch protection
with `lint`/`cargo-test` still queued.

This script is the ONE place that policy lives. The workflow's `plan` job runs
it and every other job is `needs: plan` + `if: fromJSON(plan).jobs.<name>`. The
policy is therefore testable (`--self-test`) and printable (`--table`), and a
change to "what gates a PR" is a diff to this file, not an archaeology of
`if:` blocks.

THE THREE TIERS
---------------
  pr     every pull_request push. Small, fast, deterministic. Everything in it
         must be green on `main`, because its fan-in (`pr-gate`) is the ONLY
         required status context. Budget: <= ~12 jobs, <= ~30 min wall.
  sweep  every push to `main`, coalesced (at most one running + one pending, so
         a burst of merges is tested at its tip). The PR tier unscoped, plus the
         medium-weight jobs that do not fit the PR budget (Windows builds,
         compiler-output gates, the full GC x repsel matrix, ...).
  full   nightly, release tags, `workflow_dispatch` (the release pipeline's
         `await-tests` dispatches this and waits for the `full-suite-gate` job),
         and PRs carrying the `run-extended-tests` label. The sweep plus the
         slow/opt-in suites (parity, compile-smoke, doc-tests, package smokes,
         the gap suite in its 8-shard auto-optimize mode).

PR SCOPE
--------
Within the `pr` tier the changed-file list further narrows the plan:
  docs-only  -> lint only (fmt/markdown/audits still run; nothing compiles)
  core       -> anything that can change the compiler, runtime, or the tests
                themselves; runs the whole PR tier
  deps       -> a lockfile / manifest / policy change; additionally runs the
                security-audit workflow (cargo-audit, cargo-deny, soak gate,
                agent + skills scans)

A job that is OFF in the plan is `skipped` in Actions. A skipped job counts as
satisfied for a required status check, and `pr-gate` (the fan-in) checks
`needs.plan.result == 'success'` explicitly, so a broken plan step fails the PR
instead of silently skipping everything.

Usage:
  scripts/ci_plan.py --event pull_request --ref refs/pull/1/merge \
      --changed-files files.txt [--labels a,b]        # -> JSON on stdout
  scripts/ci_plan.py --event push --ref refs/heads/main
  scripts/ci_plan.py --event workflow_dispatch --ref refs/heads/x --tier full
  scripts/ci_plan.py --table                          # markdown tier table
  scripts/ci_plan.py --self-test
When GITHUB_OUTPUT is set the JSON is also written there as `plan=<json>`.
"""

from __future__ import annotations

import argparse
import fnmatch
import json
import os
import sys

# ---------------------------------------------------------------------------
# Jobs, in workflow order. Each entry: (job id in test.yml, tiers it runs in).
# `pr` here means "runs in the PR tier when the PR is in `core` scope";
# `lint` is the one job that also runs for docs-only PRs (see plan()).
# ---------------------------------------------------------------------------
JOBS: dict[str, tuple[str, ...]] = {
    "lint": ("pr", "sweep", "full"),
    "check": ("pr", "sweep", "full"),
    "warnings": ("pr", "sweep", "full"),
    "cargo_test": ("pr", "sweep", "full"),
    "gap_suite": ("pr", "sweep", "full"),
    "gc_stress": ("pr", "sweep", "full"),
    "e2e_scoped": ("pr",),  # scoped to the PR's diff; meaningless without one
    "security_audit": ("pr", "sweep", "full"),  # pr: only when `deps` changed
    "windows_build": ("sweep", "full"),
    "windows_arm64_build": ("sweep", "full"),
    "compiler_output_regression": ("sweep", "full"),
    "repsel_census": ("sweep", "full"),
    "harmonyos_smoke": ("sweep", "full"),
    "binary_size": ("full",),  # report-only, macOS: not worth a scarce mac slot per merge
    "parity": ("full",),
    "compile_smoke": ("full",),
    "native_abi_evidence_packet": ("full",),
    "drizzle_mysql_smoke": ("full",),
    "ink_link_smoke": ("full",),
    "effect_basic_smoke": ("full",),
    "doc_tests": ("full",),
}

TIERS = ("pr", "sweep", "full")

# The gap suite: shard count and harness mode per tier.
#   fast = PERRY_SKIP_BUILD=1 against one prebuilt release compiler + runtime
#          archives; ~1.5 s/test. Only ext-routed tests (http/net/ws/zlib/
#          events) still take the per-test auto-optimize path.
#   full = the harness's default: every test compiles through auto-optimize,
#          which rebuilds a feature-stripped runtime per distinct feature set
#          (~200 s each, redundantly per shard). ~40 min/shard. This is the
#          arm that sees auto-optimize-only link bugs, so it stays in the
#          nightly/release tier.
GAP_SUITE = {
    "pr": {"mode": "fast", "total": 4},
    "sweep": {"mode": "fast", "total": 2},
    "full": {"mode": "full", "total": 8},
}

EXTENDED_LABEL = "run-extended-tests"

# ---------------------------------------------------------------------------
# PR scope classification.
# ---------------------------------------------------------------------------
# Paths that cannot change the compiler, the runtime, or any test outcome.
# A PR touching ONLY these is docs-only. Anchored globs; `**` crosses `/`.
NON_CORE_GLOBS = (
    "docs/**",
    "*.md",
    "**/*.md",
    "LICENSE*",
    ".gitignore",
    ".gitattributes",
    ".editorconfig",
    "gc-handoff/**",
    "benchmarks/**",
    "homebrew/**",
    "packaging/**",
    "npm/**",
    ".claude/**",
    "skills/**",
    ".github/**",  # re-included below: the gate's own file and shared actions
    "test-compat/**",  # node-core corpus, driven by its own scheduled workflow
    "web/**",
    "www/**",
)
# Exceptions to NON_CORE_GLOBS: these ARE core even though the glob above
# would exclude them.
CORE_OVERRIDES = (
    ".github/workflows/test.yml",
    ".github/workflows/security-audit.yml",
    ".github/actions/**",
    "docs/api/**",  # generated API docs are checked for drift by `check`
    "docs/src/api/**",
    "CLAUDE.md",  # lint's doc-claim audits read it
)

# A change here can alter the dependency graph or the supply-chain policy,
# so the security-audit workflow (cargo-audit / cargo-deny / soak gate /
# agent + skills scans) joins the PR tier.
DEPS_GLOBS = (
    "Cargo.lock",
    "Cargo.toml",
    "**/Cargo.toml",
    "deny.toml",
    "rust-toolchain",
    "rust-toolchain.toml",
    "package.json",
    "package-lock.json",
    ".npmrc",
    "external-tools.json",
    ".github/dependabot.yml",
    ".github/workflows/security-audit.yml",
    "scripts/soak/**",
    "npm/**",
    ".claude/**",
    "skills/**",
)


def _match(path: str, globs: tuple[str, ...]) -> bool:
    # fnmatch's `*` matches `/` too, so `docs/**` covers every depth and
    # `*.md` already matches `crates/x/README.md`; the `**/` spellings are
    # kept for readers used to gitignore semantics.
    return any(fnmatch.fnmatchcase(path, g) for g in globs)


def is_core(path: str) -> bool:
    if _match(path, CORE_OVERRIDES):
        return True
    return not _match(path, NON_CORE_GLOBS)


def classify(changed: list[str]) -> dict[str, bool]:
    changed = [p.strip() for p in changed if p.strip()]
    core = any(is_core(p) for p in changed)
    deps = any(_match(p, DEPS_GLOBS) for p in changed)
    return {
        "docs_only": bool(changed) and not core,
        "core": core,
        "deps": deps,
        # An empty list means the file listing failed or the PR is empty.
        # Treat it as core: silently skipping the whole tier is the failure
        # mode this file exists to prevent.
        "unknown": not changed,
    }


# ---------------------------------------------------------------------------
# Tier derivation.
# ---------------------------------------------------------------------------
def derive_tier(event: str, ref: str, labels: list[str], tier_input: str | None) -> str:
    if event == "pull_request":
        return "full" if EXTENDED_LABEL in labels else "pr"
    if event == "push":
        if ref == "refs/heads/main":
            return "sweep"
        if ref.startswith("refs/tags/"):
            return "full"
        # A push to any other branch does not trigger the workflow (see
        # `on.push.branches`), but be explicit if one ever does.
        return "sweep"
    if event == "schedule":
        return "full"
    if event == "workflow_dispatch":
        return tier_input or "full"
    raise SystemExit(f"ci_plan: unsupported event {event!r}")


def plan(
    event: str,
    ref: str,
    labels: list[str] | None = None,
    changed: list[str] | None = None,
    tier_input: str | None = None,
    update_gap_snapshot: bool = False,
) -> dict:
    labels = labels or []
    tier = derive_tier(event, ref, labels, tier_input)
    if event == "pull_request":
        scope = classify(changed or [])
        if scope["unknown"]:
            scope["core"] = True
    else:
        scope = {"docs_only": False, "core": True, "deps": True, "unknown": False}

    jobs: dict[str, bool] = {}
    for job, tiers in JOBS.items():
        on = tier in tiers
        if tier == "pr":
            if job == "lint":
                on = True
            elif job == "security_audit":
                on = scope["deps"]
            else:
                on = on and scope["core"]
        jobs[job] = on

    gap = dict(GAP_SUITE[tier])
    if update_gap_snapshot:
        # Re-baselining needs the WHOLE suite in one report so the snapshot
        # is written from a single consistent run. Fast mode: that is the arm
        # the PR gate measures against.
        gap = {"mode": "fast", "total": 1}
    gap["shards"] = list(range(1, gap["total"] + 1))
    gap["update_snapshot"] = bool(update_gap_snapshot)

    return {
        "tier": tier,
        "event": event,
        "scope": scope,
        "jobs": jobs,
        "gap": gap,
        # cargo-test: the PR tier scopes to the diff via ci_test_scope.py; the
        # other tiers run the full workspace.
        "cargo_test_scope": "pr" if tier == "pr" else "full",
        # gc-stress: the PR subset of the GC x repsel matrix vs the full one.
        "gc_stress_mode": "pr" if tier == "pr" else "full",
    }


# ---------------------------------------------------------------------------
# Presentation.
# ---------------------------------------------------------------------------
def table() -> str:
    lines = ["| job | pr | sweep | full |", "|---|:-:|:-:|:-:|"]
    for job, tiers in JOBS.items():
        cells = []
        for t in TIERS:
            mark = "yes" if t in tiers else ""
            if t == "pr" and job == "security_audit" and mark:
                mark = "deps only"
            if t == "pr" and job == "lint":
                mark = "always"
            if job == "gap_suite" and mark:
                g = GAP_SUITE[t]
                mark = f"{g['total']}x {g['mode']}"
            cells.append(mark)
        lines.append(f"| `{job.replace('_', '-')}` | " + " | ".join(cells) + " |")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Self-test: the policy must be able to fail, and the invariants CLAUDE.md
# cares about must hold.
# ---------------------------------------------------------------------------
def _self_test() -> int:
    failures: list[str] = []

    def check(name: str, cond: bool):
        if not cond:
            failures.append(name)

    # 1. Tiers.
    check("PR is pr", derive_tier("pull_request", "refs/pull/1/merge", [], None) == "pr")
    check("labelled PR is full", derive_tier("pull_request", "refs/pull/1/merge", [EXTENDED_LABEL], None) == "full")
    check("push main is sweep", derive_tier("push", "refs/heads/main", [], None) == "sweep")
    check("tag is full", derive_tier("push", "refs/tags/v0.5.9999", [], None) == "full")
    check("schedule is full", derive_tier("schedule", "refs/heads/main", [], None) == "full")
    check("dispatch default is full", derive_tier("workflow_dispatch", "refs/heads/x", [], None) == "full")
    check("dispatch tier honoured", derive_tier("workflow_dispatch", "refs/heads/x", [], "sweep") == "sweep")

    # 2. PR scope.
    docs = plan("pull_request", "refs/pull/1/merge", changed=["docs/src/foo.md", "README.md"])
    check("docs-only PR: lint on", docs["jobs"]["lint"])
    check("docs-only PR: gap suite off", not docs["jobs"]["gap_suite"])
    check("docs-only PR: cargo-test off", not docs["jobs"]["cargo_test"])
    check("docs-only PR: security-audit off", not docs["jobs"]["security_audit"])
    check("docs-only PR flagged", docs["scope"]["docs_only"])

    core = plan("pull_request", "refs/pull/1/merge", changed=["crates/perry-runtime/src/gc/mod.rs"])
    check("core PR: gap suite on", core["jobs"]["gap_suite"])
    check("core PR: gc-stress on", core["jobs"]["gc_stress"])
    check("core PR: e2e-scoped on", core["jobs"]["e2e_scoped"])
    check("core PR: windows off", not core["jobs"]["windows_build"])
    check("core PR: parity off", not core["jobs"]["parity"])
    check("core PR: security-audit off (no deps change)", not core["jobs"]["security_audit"])
    check("core PR: 4 fast gap shards", core["gap"] == {"mode": "fast", "total": 4, "shards": [1, 2, 3, 4], "update_snapshot": False})
    check("core PR: cargo-test scoped", core["cargo_test_scope"] == "pr")

    deps = plan("pull_request", "refs/pull/1/merge", changed=["Cargo.lock"])
    check("deps PR: security-audit on", deps["jobs"]["security_audit"])
    check("deps PR: core (lockfile changes the build)", deps["jobs"]["cargo_test"])

    empty = plan("pull_request", "refs/pull/1/merge", changed=[])
    check("empty listing is treated as core", empty["jobs"]["gap_suite"] and empty["scope"]["unknown"])

    # The gate's own wiring and the shared actions are core even though the
    # rest of .github/ is not.
    check("test.yml change is core", is_core(".github/workflows/test.yml"))
    check("shared action change is core", is_core(".github/actions/setup-llvm22/action.yml"))
    check("other workflow change is not core", not is_core(".github/workflows/benchmark.yml"))
    check("nested .md is not core", not is_core("crates/perry/README.md"))
    check("CLAUDE.md is core (lint doc-claim audits)", is_core("CLAUDE.md"))
    check("a .ts test file is core", is_core("test-files/test_gap_x.ts"))
    check("scripts are core", is_core("scripts/run_gap_tests.sh"))
    check("generated api docs are core", is_core("docs/api/perry.d.ts"))

    # 3. Sweep and full.
    sweep = plan("push", "refs/heads/main")
    check("sweep: windows on", sweep["jobs"]["windows_build"] and sweep["jobs"]["windows_arm64_build"])
    check("sweep: binary-size off (macOS report-only, nightly is enough)", not sweep["jobs"]["binary_size"])
    check("sweep: parity off", not sweep["jobs"]["parity"])
    check("sweep: e2e-scoped off", not sweep["jobs"]["e2e_scoped"])
    check("sweep: 2 fast gap shards", sweep["gap"]["total"] == 2 and sweep["gap"]["mode"] == "fast")
    check("sweep: cargo-test full", sweep["cargo_test_scope"] == "full")
    check("sweep: security-audit on", sweep["jobs"]["security_audit"])

    full = plan("schedule", "refs/heads/main")
    check("full: every job except e2e-scoped", all(v for k, v in full["jobs"].items() if k != "e2e_scoped"))
    check("full: 8 auto-optimize gap shards", full["gap"]["total"] == 8 and full["gap"]["mode"] == "full")

    labelled = plan("pull_request", "refs/pull/1/merge", labels=[EXTENDED_LABEL], changed=["README.md"])
    check("labelled PR runs the full tier regardless of scope", labelled["jobs"]["parity"] and labelled["jobs"]["gap_suite"])

    snap = plan("workflow_dispatch", "refs/heads/x", tier_input="pr", update_gap_snapshot=True)
    check("snapshot update: one fast shard", snap["gap"] == {"mode": "fast", "total": 1, "shards": [1], "update_snapshot": True})

    # 4. The gc_gate_wiring_check contract: `gc-stress` is a registered
    #    moving-GC gate and MUST be main-line reachable (push:main or
    #    schedule). The checker cannot see through fromJSON(plan), so this is
    #    where that guarantee lives.
    check("gc-stress reachable on push:main", sweep["jobs"]["gc_stress"] and sweep["gc_stress_mode"] == "full")
    check("gc-stress reachable on schedule", full["jobs"]["gc_stress"] and full["gc_stress_mode"] == "full")

    # 5. Sabotage: the checker can fail.
    check("sabotage: a job cannot be in no tier", all(JOBS.values()))
    check("sabotage: unknown event raises", _raises(lambda: derive_tier("issue_comment", "x", [], None)))

    if failures:
        print("ci_plan --self-test FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print(f"ci_plan --self-test: {len(JOBS)} jobs, 3 tiers, all invariants hold")
    return 0


def _raises(fn) -> bool:
    try:
        fn()
    except SystemExit:
        return True
    return False


def main(argv: list[str]) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--event")
    ap.add_argument("--ref", default="")
    ap.add_argument("--labels", default="", help="comma-separated PR label names")
    ap.add_argument("--changed-files", help="file with one changed path per line (PR only)")
    ap.add_argument("--tier", choices=TIERS, help="workflow_dispatch tier input")
    ap.add_argument("--update-gap-snapshot", action="store_true")
    ap.add_argument("--table", action="store_true", help="print the tier table as markdown")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args(argv)

    if args.self_test:
        return _self_test()
    if args.table:
        print(table())
        return 0
    if not args.event:
        ap.error("--event is required")

    changed: list[str] = []
    if args.changed_files:
        with open(args.changed_files, encoding="utf-8") as fh:
            changed = [ln.rstrip("\n") for ln in fh if ln.strip()]
    labels = [s.strip() for s in args.labels.split(",") if s.strip()]

    p = plan(
        args.event,
        args.ref,
        labels=labels,
        changed=changed,
        tier_input=args.tier,
        update_gap_snapshot=args.update_gap_snapshot,
    )
    out = json.dumps(p, separators=(",", ":"), sort_keys=True)
    print(json.dumps(p, indent=2, sort_keys=True))
    gh_out = os.environ.get("GITHUB_OUTPUT")
    if gh_out:
        with open(gh_out, "a", encoding="utf-8") as fh:
            fh.write(f"plan={out}\n")
            fh.write(f"tier={p['tier']}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
