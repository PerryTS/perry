#!/usr/bin/env python3
"""A merge may raise a resolved dependency version. It may never lower one.

WHY THIS EXISTS (#10980)
------------------------
`aeaa912c1d` bumped rustls to 0.23.45 for RUSTSEC-2026-0285. A branch that
CONTAINS that commit resolves rustls to 0.23.44, because a merge regenerated
`Cargo.lock`: the fix is in the history and gone from the artifact that decides
what gets built.

Nothing could have caught it. `Cargo.lock` appears in `.github/workflows` only
as a cache key (`hashFiles('**/Cargo.lock')`); no job asserts on its contents.
It determines every built artifact, it is rewritten by tooling during merges,
and no test can fail when it moves the wrong way.

THE PREDICATE, AND THE ONE IT REPLACED
--------------------------------------
PER-CONSUMER. A dependency D is downgraded only when some consumer C present in
BOTH locks resolves D to a lower version in the new one.

The first version of this gate asked `max(after) >= max(before)` per package,
and that predicate **cannot distinguish a downgrade from a removal**:

* rustls 0.23.45 -> 0.23.44: the same consumers exist on both sides and each
  resolves lower. A real downgrade.
* crc 3.4.0 -> 2.1.0 on the same pair: crc@3.4.0's only consumers were
  `sqlx-core` / `sqlx-mysql` / `sqlx-postgres`, and that branch deletes the
  pg/mysql2 bindings, so they are gone. The single shared consumer,
  `swc_bundler`, resolves crc@2.1.0 on BOTH sides. Nothing moved backwards --
  the higher version left with its only consumers.

On the real locks the old predicate reported three findings of which **two were
false**, and both would have blocked a correct merge. A gate has to be
trustworthy when it goes RED, not only when it is green; one that blocks
correct work gets bypassed, and then it protects nothing.

"Contains the fix and resolves below it" is a good line for a diagnostic and it
is NOT the test -- the crc case contains nothing and resolves nothing backwards,
and the old rule fired on it anyway.

THE TRAP INSIDE THE FIX
-----------------------
Edges are keyed on (consumer NAME, consumer VERSION). Keying on the name alone
recreates the same conflation one level down: main carries crc@2.1.0 AND
crc@3.4.0 wanting different `crc-catalog`s, so a name-keyed comparison reports
"crc downgraded crc-catalog 2.5.0 -> 1.1.1" when crc@2.1.0 -> crc-catalog@1.1.1
is identical on both sides.

A name-level fallback then applies ONLY to consumers whose own version changed
between the locks (no version in common), so a bumped consumer cannot hide a
real downgrade behind its own bump.

WHAT IS AND IS NOT A FAILURE
----------------------------
* downgraded via a shared consumer  -> FAIL
* removed along with its consumers  -> REPORTED, never failed
* upgrade / unchanged / added       -> pass

One direction only. A downgrade is never what a merge intends, so there is
nothing to allow; an upgrade routinely is, so gating it would need a baseline
and an allowlist, and a gate with an allowlist is one people learn to edit.
No advisory database: this never asks "is this version vulnerable?", only "did
this merge move a pin backwards?" -- which is what makes it catch the general
case rather than one CVE.

Registry dependencies only (`source = "registry+`). A workspace member's
version comes from the release bump rather than the resolver, and a git or path
dependency has no resolver-ordered version to compare.

USAGE
-----
    scripts/lock_no_downgrade.py --before OLD.lock --after NEW.lock
    scripts/lock_no_downgrade.py --vs <git-ref>     # ref's lock vs the tree's
    scripts/lock_no_downgrade.py --self-test        # proves it can fail
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
LOCK = REPO_ROOT / "Cargo.lock"

_NAME = re.compile(r'^name = "([^"]+)"', re.M)
_VERSION = re.compile(r'^version = "([^"]+)"', re.M)
_REGISTRY = re.compile(r'^source = "registry\+', re.M)
_DEPS = re.compile(r"^dependencies = \[(.*?)\]", re.M | re.S)


def parse_lock(text: str):
    """[(name, version, is_registry, [dep strings])]"""
    out = []
    for block in text.split("[[package]]")[1:]:
        name = _NAME.search(block)
        version = _VERSION.search(block)
        if not (name and version):
            continue
        deps_match = _DEPS.search(block)
        deps = re.findall(r'"([^"]+)"', deps_match.group(1)) if deps_match else []
        out.append((name.group(1), version.group(1),
                    bool(_REGISTRY.search(block)), deps))
    return out


def version_key(v: str):
    """Order a lock version string, with a pre-release BELOW its release.

    Deliberately not the reference implementation's flat
    `re.split(r"[.+-]", v)`: that makes `1.2.3-alpha` sort ABOVE `1.2.3`,
    because the extra components make the pre-release the longer tuple. The
    real locks never exercised it -- their pre-release findings
    (`pkcs1 0.8.0-rc.4 -> 0.7.5`, `rsa 0.10.0-rc.18 -> 0.9.10`) are downgrades
    on the numeric part alone -- so it was invisible to a proof run against
    them, and my own self-test is what caught it. Semver says
    `1.2.3-rc.1 < 1.2.3`, and shipping a release candidate over a release is
    exactly the backwards move this gate exists to refuse.

    Build metadata after `+` is dropped: semver says it is not
    ordering-significant.

    Non-numeric components sort after numeric ones rather than raising. A
    version this cannot order must not take the gate down, because then its
    failure mode is 'everything is blocked'.
    """
    core, _, pre = v.partition("-")
    parts = [(0, int(p)) if p.isdigit() else (1, p)
             for p in core.split("+")[0].split(".")]
    if not pre:
        return (parts, 1, [])
    pre_parts = [(0, int(p)) if p.isdigit() else (1, p)
                 for p in pre.split("+")[0].split(".")]
    return (parts, 0, pre_parts)


def index(pkgs):
    versions, registry = {}, set()
    for name, version, is_registry, _ in pkgs:
        versions.setdefault(name, set()).add(version)
        if is_registry:
            registry.add((name, version))
    return versions, registry


def resolve(dep: str, versions):
    """'crc' / 'crc 2.1.0' / 'crc 2.1.0 (registry+...)' -> (name, version|None).

    A bare name is only resolvable when the lock carries exactly one version of
    it; otherwise this returns None and the edge is skipped rather than guessed.
    """
    parts = dep.split()
    if len(parts) > 1:
        return parts[0], parts[1]
    candidates = versions.get(parts[0], set())
    return parts[0], (next(iter(candidates)) if len(candidates) == 1 else None)


def edges(pkgs, versions, registry):
    """{(consumer_name, consumer_version, dep_name): resolved registry version}"""
    out = {}
    for cname, cversion, _is_registry, deps in pkgs:
        for dep in deps:
            dname, dversion = resolve(dep, versions)
            if dversion is None or (dname, dversion) not in registry:
                continue
            k = (cname, cversion, dname)
            if k not in out or version_key(dversion) > version_key(out[k]):
                out[k] = dversion
    return out


def analyse(before_text: str, after_text: str):
    """(downgrades, removed_with_consumers, comparisons_made)"""
    base, new_pkgs = parse_lock(before_text), parse_lock(after_text)
    bver, breg = index(base)
    nver, nreg = index(new_pkgs)
    be, ne = edges(base, bver, breg), edges(new_pkgs, nver, nreg)
    new_pairs = {(n, v) for n, v, *_ in new_pkgs}

    downs = set()
    # `compared` counts COMPARISONS ACTUALLY MADE, not edges parsed from the
    # before-lock. Those are different populations and the difference is the
    # whole value of the number: if the after-lock parses to nothing, the
    # before-edges are still there, no downgrade is found, and a count of
    # before-edges would report a large, reassuring denominator for a run that
    # compared zero things. A denominator is only a denominator if it counts
    # the thing being claimed.
    compared = 0

    # 1. the same consumer, at the same version, on both sides
    for (cname, cversion, dname), bv in be.items():
        nv = ne.get((cname, cversion, dname))
        if nv is None:
            continue
        compared += 1
        if version_key(nv) < version_key(bv):
            downs.add((dname, f"{cname} {cversion}", bv, nv))

    # 2. a consumer VERSION that is gone while a HIGHER version of the same
    #    consumer exists in the new lock — i.e. it was BUMPED. There is no
    #    exact key to compare, so fall back to what the newer version resolves,
    #    otherwise bumping a consumer hides the downgrade it brought with it.
    #
    #    PER VERSION, not per name: gating on "the consumer's version sets are
    #    disjoint" misses a PARTIAL change. With app@1.0 + app@2.0 before and
    #    app@2.0 + app@3.0 after, the sets intersect at 2.0, so a name-level
    #    guard skips the consumer entirely and the 1.0 -> 3.0 move is compared
    #    by neither rule.
    #
    #    "A HIGHER version exists" is what separates a bump from a DROP, and
    #    that distinction is the crc case again one level in: before carries
    #    crc@2.1.0 + crc@3.4.0, after carries only crc@2.1.0. crc@3.4.0 did not
    #    become anything — it left, taking crc-catalog@2.5.0 with it. Comparing
    #    its edges against crc@2.1.0's would report the same false downgrade
    #    this rewrite exists to remove.
    for (cname, cversion, dname), bv in be.items():
        if (cname, cversion) in new_pairs:
            continue          # rule 1 owns this one
        successors = [v for v in nver.get(cname, set())
                      if version_key(v) > version_key(cversion)]
        if not successors:
            continue          # dropped, not bumped: a removal
        resolutions = [v for (c, cv, d), v in ne.items()
                       if c == cname and d == dname and cv in successors]
        if not resolutions:
            continue          # the newer consumer no longer needs it at all
        compared += 1
        # ANY successor resolving lower is worth a human: a newly introduced
        # consumer version that pulls an older dependency is a backwards move,
        # even when an older sibling still holds the newer one.
        worst = min(resolutions, key=version_key)
        if version_key(worst) < version_key(bv):
            downs.add((dname, f"{cname} {cversion} -> bumped", bv, worst))

    removed = sorted({(dname, bv) for (cname, cversion, dname), bv in be.items()
                      if (cname, cversion) not in new_pairs})
    return sorted(downs), removed, compared


def report(downs, removed, edges_checked: int) -> int:
    if removed:
        shown = ", ".join(f"{n} {v}" for n, v in removed[:12])
        print(f"removed with their consumers (reported, not failed): {shown}"
              + (" ..." if len(removed) > 12 else ""))
    if not downs:
        # The denominator is not decoration. A gate that prints "clean" without
        # saying how much it examined cannot be told apart from one that
        # examined nothing -- an unfetched ref, an unparsable lock, a filter
        # that excluded everything. #10944 was exactly that failure at suite
        # scale, so this one says its own coverage.
        print("lock downgrade gate: no resolved version moved backwards "
              f"({edges_checked} dependency edges compared across both locks)")
        return 0
    print("LOCKFILE DOWNGRADE — refusing:", file=sys.stderr)
    for dname, via, bv, nv in downs:
        print(f"  {dname}: {bv} -> {nv}   (via shared consumer {via})",
              file=sys.stderr)
    print(
        f"\n{len(downs)} resolved dependency version(s) moved BACKWARDS for a "
        "consumer present on both sides. A merge may raise a pin and may never "
        "lower one: regenerating `Cargo.lock` is how a landed fix comes undone "
        "without any test failing (#10980 — rustls 0.23.45 -> 0.23.44 with the "
        "RUSTSEC bump already in history). Re-resolve with the newer pin, or "
        "say in the merge why the older one is correct.",
        file=sys.stderr,
    )
    return 1


def git_show(ref: str, path: str):
    r = subprocess.run(["git", "show", f"{ref}:{path}"],
                       capture_output=True, text=True, cwd=REPO_ROOT, check=False)
    return r.stdout if r.returncode == 0 else None


def self_test() -> int:
    """A gate that cannot fail is documentation — and one that fires on correct
    work is worse than none. These cases were derived from the REAL locks
    first; the earlier suite had twelve greens on a predicate that mis-fired on
    two of its three real findings, because it only covered the shapes the
    predicate was designed for.
    """
    def pkg(name, version, deps=(), registry=True):
        src = ('source = "registry+https://github.com/rust-lang/'
               'crates.io-index"\n') if registry else ""
        dep_lines = ("dependencies = [\n"
                     + "".join(f' "{d}",\n' for d in deps) + "]\n") if deps else ""
        return f'[[package]]\nname = "{name}"\nversion = "{version}"\n{src}{dep_lines}\n'

    cases = []

    # THE REAL DOWNGRADE: one shared consumer resolving lower on both sides.
    cases.append(("a shared consumer resolving lower is caught",
        pkg("app", "1.0.0", ["rustls 0.23.45"], registry=False) + pkg("rustls", "0.23.45"),
        pkg("app", "1.0.0", ["rustls 0.23.44"], registry=False) + pkg("rustls", "0.23.44"),
        1))

    # THE REAL FALSE POSITIVE the old predicate produced, and the case whose
    # ABSENCE from the first suite is why that predicate shipped: a higher
    # version whose every consumer is gone from the new lock.
    cases.append(("a higher version whose every consumer is ABSENT is a removal, not a downgrade",
        pkg("sqlx-core", "0.8.2", ["crc 3.4.0"], registry=False)
        + pkg("bundler", "1.0.0", ["crc 2.1.0"], registry=False)
        + pkg("crc", "3.4.0") + pkg("crc", "2.1.0"),
        pkg("bundler", "1.0.0", ["crc 2.1.0"], registry=False) + pkg("crc", "2.1.0"),
        0))

    # The trap one level down: two versions of one consumer wanting different
    # versions of the same dependency, unchanged on both sides.
    cases.append(("two versions of one consumer wanting different deps is not a downgrade",
        pkg("crc", "3.4.0", ["crc-catalog 2.5.0"]) + pkg("crc", "2.1.0", ["crc-catalog 1.1.1"])
        + pkg("crc-catalog", "2.5.0") + pkg("crc-catalog", "1.1.1"),
        pkg("crc", "2.1.0", ["crc-catalog 1.1.1"]) + pkg("crc-catalog", "1.1.1"),
        0))

    # A bumped consumer must not be able to hide a downgrade behind its bump.
    cases.append(("a consumer whose own version changed cannot hide a downgrade",
        pkg("app", "1.0.0", ["rustls 0.23.45"], registry=False) + pkg("rustls", "0.23.45"),
        pkg("app", "2.0.0", ["rustls 0.23.44"], registry=False) + pkg("rustls", "0.23.44"),
        1))

    for label, extra_before, extra_after, expect in [
        ("upgrade passes",
         pkg("app", "1.0.0", ["r 1.0.0"], registry=False) + pkg("r", "1.0.0"),
         pkg("app", "1.0.0", ["r 1.1.0"], registry=False) + pkg("r", "1.1.0"), 0),
        ("unchanged passes",
         pkg("app", "1.0.0", ["r 1.0.0"], registry=False) + pkg("r", "1.0.0"),
         pkg("app", "1.0.0", ["r 1.0.0"], registry=False) + pkg("r", "1.0.0"), 0),
        ("a new dependency passes",
         pkg("app", "1.0.0", ["keep 1.0.0"], registry=False) + pkg("keep", "1.0.0"),
         pkg("app", "1.0.0", ["keep 1.0.0", "r 1.0.0"], registry=False)
         + pkg("keep", "1.0.0") + pkg("r", "1.0.0"), 0),
        # A stable third consumer supplies the compared edge, so this case
        # tests "the workspace bump does not fire" rather than "nothing was
        # comparable" — which is a different, vacuous claim.
        ("a workspace member moving backwards is ignored",
         pkg("perry", "0.5.1635", ["keep 1.0.0"], registry=False)
         + pkg("stable", "1.0.0", ["keep 1.0.0"], registry=False) + pkg("keep", "1.0.0"),
         pkg("perry", "0.5.1634", ["keep 1.0.0"], registry=False)
         + pkg("stable", "1.0.0", ["keep 1.0.0"], registry=False) + pkg("keep", "1.0.0"), 0),
        ("a pre-release is below its release",
         pkg("app", "1.0.0", ["r 1.2.3"], registry=False) + pkg("r", "1.2.3"),
         pkg("app", "1.0.0", ["r 1.2.3-alpha.1"], registry=False) + pkg("r", "1.2.3-alpha.1"), 1),
        ("an unorderable component does not take the gate down",
         pkg("app", "1.0.0", ["r 1.x.3"], registry=False) + pkg("r", "1.x.3"),
         pkg("app", "1.0.0", ["r 1.x.3"], registry=False) + pkg("r", "1.x.3"), 0),
    ]:
        cases.append((label, extra_before, extra_after, expect))

    # (label, before, after, expect_fires, expect_compared_zero)
    cases = [(c + (False,))[:5] for c in cases]

    # The two REVIEW FINDINGS, as cases. Both were false negatives — the
    # direction that matters, because a gate that misses a downgrade is worse
    # than no gate: it is believed. Neither was reachable by the twelve cases
    # that preceded them, for the third time today the same way: the
    # population was wrong, not the polarity.
    cases.append((
        "an after-lock with NO registry edges must FAIL, not pass — the "
        "denominator has to count comparisons made, not before-edges parsed",
        pkg("app", "1.0.0", ["r 1.0.0"], registry=False) + pkg("r", "1.0.0"),
        pkg("app", "1.0.0", [], registry=False),
        0, True))
    cases.append((
        "a PARTIALLY overlapping consumer cannot hide a downgrade behind its "
        "bumped version",
        pkg("app", "1.0.0", ["r 2.0.0"], registry=False)
        + pkg("app", "2.0.0", ["r 2.0.0"], registry=False) + pkg("r", "2.0.0"),
        pkg("app", "2.0.0", ["r 2.0.0"], registry=False)
        + pkg("app", "3.0.0", ["r 1.0.0"], registry=False)
        + pkg("r", "2.0.0") + pkg("r", "1.0.0"),
        1, False))

    failures = 0
    for label, before, after, expect, expect_zero in cases:
        downs, _removed, compared = analyse(before, after)
        got = 1 if downs else 0
        if got != expect:
            print(f"self-test FAILED: {label} (expected {expect}, got {got})",
                  file=sys.stderr)
            failures += 1
        if expect_zero and compared != 0:
            print(f"self-test FAILED: {label} (expected 0 comparisons, "
                  f"got {compared})", file=sys.stderr)
            failures += 1
        if not expect_zero and compared == 0:
            print(f"self-test FAILED: {label} compared NOTHING, so its verdict "
                  "is vacuous", file=sys.stderr)
            failures += 1
    if failures:
        return 1
    print(f"lock downgrade self-test: {len(cases)} cases — catches a real "
          "downgrade, and does not fire on a removal, a second consumer "
          "version, or a workspace bump")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--before")
    parser.add_argument("--after")
    parser.add_argument("--vs", metavar="REF",
                        help="compare REF's Cargo.lock against the tree's")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if args.vs:
        before_text = git_show(args.vs, "Cargo.lock")
        if before_text is None:
            print(f"::error::cannot resolve {args.vs}:Cargo.lock. The merge base "
                  "was not fetched, so this gate would pass by finding nothing "
                  "to compare — which is the failure mode it exists to remove.",
                  file=sys.stderr)
            return 1
        after_text = LOCK.read_text(encoding="utf-8")
    elif args.before and args.after:
        before_text = Path(args.before).read_text(encoding="utf-8")
        after_text = Path(args.after).read_text(encoding="utf-8")
    else:
        parser.error("need --vs REF, or both --before and --after")

    downs, removed, edges_checked = analyse(before_text, after_text)
    if edges_checked == 0:
        print("::error::0 dependency edges were COMPARED across the two locks. "
              "Either a lock failed to parse, or one side has no registry "
              "dependencies, or the filter excluded everything. A gate that "
              "checks nothing and reports success is the failure mode this "
              "exists to remove.", file=sys.stderr)
        return 1
    return report(downs, removed, edges_checked)


if __name__ == "__main__":
    sys.exit(main())
