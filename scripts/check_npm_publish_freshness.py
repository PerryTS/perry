#!/usr/bin/env python3
"""Fail when what npm serves as `latest` has fallen behind this checkout.

#7491: a user compiling with the npm build hit `unresolved external symbol
js_ext_http_client_request_is_handle` at link time. The fix had been on `main` for
weeks. The version npm was serving as `latest` (0.5.1220) was **a month old**, and
nothing anywhere said so -- the user found it, from the outside, by reading the
versions tab.

Every gate in this repo was green throughout, and correctly so: they all measure
`main`. What a user installs is not `main`, it is whatever the last successful run
of release-packages.yml happened to leave on the registry. Nothing compared the two,
so the gap between them was unobservable until it produced a bug report. This script
is that comparison.

WHAT "STALE" MEANS HERE
-----------------------
Two budgets per package, in scripts/npm_publish_freshness.json:

  * **age of the published release** -- counted ONLY while this checkout is ahead of
    it. A month-old publish with nothing unreleased behind it is a quiet week, not a
    problem; a month-old publish with 300 merged patches behind it is #7491. Age is
    the honest signal, and it is the one that would have caught #7491 on day 15.
  * **patch distance** -- every merge to `main` bumps the workspace patch, so the
    distance is a commit count in disguise. It is a backstop for a cadence spike
    inside an age budget that has not expired yet, NOT a release-cadence rule; that
    job belongs to the age budget alone.

Plus one invariant that needs no budget: **every platform package must publish the
same `latest` as the launcher.** `npm/perry/package.json.tmpl` pins its
optionalDependencies to the exact launcher version, so a platform package left
behind by a partial publish breaks installs outright while both packages sit
comfortably inside their own budgets. That is #7491's neighbour, not its twin, and
it is one comparison away once you are already holding both packuments.

A REGISTRY ERROR IS RED, NOT A SKIP
-----------------------------------
Deliberate, and the reason is the whole point of the script. This detector exists
because a silence read as health for a month. A network failure that exits 0 is that
same silence wearing a CI badge -- CLAUDE.md hazard 4, a gate that runs while its
subject never did. So: three attempts, then RED, naming the transport error. The
cost of that choice is a red run on a registry blip, which is a re-run; the cost of
the other choice is #7491 again, which is a user's afternoon.

For the same reason there is no `time.modified` fallback (see `evaluate_package`),
and a scoped `--package` run refuses to touch the sticky issue: a subset must never
be able to declare the whole set healthy.

Usage:
    python3 scripts/check_npm_publish_freshness.py --self-test      # prove it can fail
    python3 scripts/check_npm_publish_freshness.py --check-manifest # offline, no network
    python3 scripts/check_npm_publish_freshness.py --dry-run        # real registry, no issue writes
    python3 scripts/check_npm_publish_freshness.py                  # the gate
    python3 scripts/check_npm_publish_freshness.py --package @perryts/perry \
        --packument saved.json                                      # replay a saved response
"""

from __future__ import annotations

import argparse
import datetime as _dt
import json
import os
import re
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterable, Sequence

REPO_ROOT = Path(__file__).resolve().parent.parent
MANIFEST = REPO_ROOT / "scripts" / "npm_publish_freshness.json"
CARGO_TOML = REPO_ROOT / "Cargo.toml"
NPM_DIR = REPO_ROOT / "npm"
REGISTRY = "https://registry.npmjs.org"

# Marker in the sticky issue title so we update one issue forever instead of opening a
# new one every day. Changing this string orphans the existing issue.
ISSUE_MARKER = "npm publish freshness alert"

VERSION_RE = re.compile(r"^(\d+)\.(\d+)\.(\d+)$")


class RegistryError(RuntimeError):
    """The registry did not give us something we could compare against."""


# --------------------------------------------------------------------------- versions


def parse_version(text: str) -> tuple[int, int, int]:
    """Parse a plain `x.y.z`.

    Anything else -- a prerelease, a build tag, an empty string -- raises. Perry has
    never shipped a non-plain version to the `latest` dist-tag, and guessing at the
    ordering of one is how a comparison starts silently answering the wrong question.
    If that day comes, this needs an explicit rule, not a lenient regex.
    """
    m = VERSION_RE.match(text.strip())
    if not m:
        raise ValueError(f"not a plain x.y.z version: {text!r}")
    major, minor, patch = m.groups()
    return int(major), int(minor), int(patch)


def patch_distance(repo: tuple[int, int, int], published: tuple[int, int, int]) -> int | None:
    """Patch bumps `repo` is ahead of `published`, or None across an x.y boundary."""
    if repo[:2] != published[:2]:
        return None
    return repo[2] - published[2]


def workspace_version(cargo_toml: Path = CARGO_TOML) -> str:
    """The `[workspace.package] version`.

    Section-aware on purpose. A workspace Cargo.toml has many `version =` keys and
    the release workflow's `grep -m1 '^version'` gets the right one only because of
    where the section happens to sit today.
    """
    text = cargo_toml.read_text()
    section = re.search(r"^\[workspace\.package\]\s*$", text, re.M)
    if not section:
        raise RegistryError(f"no [workspace.package] section in {cargo_toml}")
    for line in text[section.end():].splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            break
        m = re.match(r'^version\s*=\s*"([^"]+)"', stripped)
        if m:
            return m.group(1)
    raise RegistryError(f"no version key under [workspace.package] in {cargo_toml}")


# --------------------------------------------------------------------------- manifest


@dataclass(frozen=True)
class Package:
    name: str
    max_unpublished_age_days: float
    max_version_distance: int
    anchor: bool
    why: str


def load_packages(manifest: Path = MANIFEST) -> list[Package]:
    data = json.loads(manifest.read_text())
    defaults = data.get("defaults", {})
    packages = [
        Package(
            name=entry["name"],
            max_unpublished_age_days=float(
                entry.get("max_unpublished_age_days", defaults["max_unpublished_age_days"])
            ),
            max_version_distance=int(
                entry.get("max_version_distance", defaults["max_version_distance"])
            ),
            anchor=bool(entry.get("anchor", False)),
            why=entry.get("why", ""),
        )
        for entry in data["packages"]
    ]
    if not packages:
        raise SystemExit(
            f"{manifest.name} lists no packages; refusing to pass vacuously"
        )
    if sum(1 for p in packages if p.anchor) != 1:
        raise SystemExit(
            f"{manifest.name} must mark exactly one package as the `anchor` -- the "
            f"launcher whose version every platform package has to match"
        )
    return packages


def npm_package_names(npm_dir: Path = NPM_DIR) -> set[str]:
    """Package names as declared by the publish templates under npm/."""
    names = set()
    for tmpl in sorted(npm_dir.glob("*/package.json.tmpl")):
        m = re.search(r'"name"\s*:\s*"([^"]+)"', tmpl.read_text())
        if not m:
            raise SystemExit(
                f"{tmpl} declares no literal `name` -- the freshness gate cannot "
                "tell which package it publishes, so it would stop covering it"
            )
        names.add(m.group(1))
    return names


def coverage_problems(shipped: set[str], watched: set[str]) -> list[str]:
    """Anti-drift: the manifest must name exactly what release-packages.yml publishes.

    A platform package added under npm/ and not added here would be published,
    installed, and never watched -- the check would stay green while covering less
    than it used to, which is a gate quietly shrinking rather than failing.
    """
    problems = []
    for missing in sorted(shipped - watched):
        problems.append(
            f"{missing} is published from npm/ but is not in {MANIFEST.name} -- "
            f"add it, or the freshness gate silently stops covering it"
        )
    for extra in sorted(watched - shipped):
        problems.append(
            f"{extra} is watched by {MANIFEST.name} but no npm/*/package.json.tmpl "
            f"declares it -- was it renamed or dropped?"
        )
    return problems


# --------------------------------------------------------------------------- registry


def fetch_packument(name: str, *, attempts: int = 3, timeout: float = 30.0) -> dict:
    """The FULL packument for `name`.

    Not the abbreviated (`application/vnd.npm.install-v1+json`) document: that one
    omits the `time` map, and `time` is where the age signal lives. No auth -- these
    are public packages and a token would make this fail differently for outside
    contributors than for CI.
    """
    url = f"{REGISTRY}/{urllib.parse.quote(name, safe='')}"
    last: Exception | None = None
    for attempt in range(1, attempts + 1):
        try:
            req = urllib.request.Request(
                url,
                headers={
                    "Accept": "application/json",
                    "User-Agent": "perry-npm-publish-freshness (PerryTS/perry)",
                },
            )
            with urllib.request.urlopen(req, timeout=timeout) as resp:
                return json.loads(resp.read().decode("utf-8"))
        except urllib.error.HTTPError as exc:
            # A 404 is an answer, not a transport failure: the registry has no such
            # package. Retrying it just makes the run slower, and calling it
            # "unreachable" sends the reader to look at the network instead of at the
            # publish step that never ran. It is still RED -- npm/ ships a template
            # for it and the launcher pins it as an optionalDependency, so a package
            # that exists in the tree and not on the registry is a broken install for
            # that platform.
            if exc.code == 404:
                raise RegistryError(
                    f"the registry has no package `{name}` at all (HTTP 404) -- it "
                    f"has never been published, or it was removed, while npm/ still "
                    f"ships a package.json.tmpl for it"
                ) from exc
            last = exc
            if attempt < attempts:
                time.sleep(2.0 * attempt)
        except (urllib.error.URLError, TimeoutError, ValueError, OSError) as exc:
            last = exc
            if attempt < attempts:
                time.sleep(2.0 * attempt)
    raise RegistryError(f"registry unreachable after {attempts} attempts ({url}): {last}")


# --------------------------------------------------------------------------- verdicts


@dataclass(frozen=True)
class Verdict:
    package: str
    published: str | None
    age_days: float | None
    distance: int | None
    problems: tuple[str, ...]

    @property
    def stale(self) -> bool:
        return bool(self.problems)

    @property
    def detail(self) -> str:
        bits = [f"latest {self.published or '?'}"]
        bits.append("age ?" if self.age_days is None else f"age {self.age_days:.1f}d")
        bits.append("dist ?" if self.distance is None else f"dist +{self.distance}")
        return ", ".join(bits)


def _parse_ts(value: str) -> _dt.datetime:
    parsed = _dt.datetime.fromisoformat(value.replace("Z", "+00:00"))
    if parsed.tzinfo is None or parsed.utcoffset() is None:
        raise ValueError(f"timestamp has no UTC offset: {value!r}")
    return parsed.astimezone(_dt.timezone.utc)


def evaluate_package(
    pkg: Package,
    repo_version: str,
    packument: dict,
    now: _dt.datetime,
    anchor_published: str | None = None,
) -> Verdict:
    problems: list[str] = []
    repo = parse_version(repo_version)

    published = (packument.get("dist-tags") or {}).get("latest")
    if not published:
        return Verdict(
            pkg.name,
            None,
            None,
            None,
            (f"the registry returned no `latest` dist-tag for {pkg.name}",),
        )

    # The subject has to be live: a dist-tag pointing at a version the packument does
    # not list is a broken publish, not a healthy one we happen not to understand.
    if published not in (packument.get("versions") or {}):
        problems.append(
            f"`latest` is {published} but that version is not in the packument's "
            f"`versions` map -- the publish did not complete"
        )

    # `time.modified` is deliberately NOT a fallback here. It moves whenever ANY
    # metadata changes -- a deprecation, a dist-tag edit, an ownership change -- so a
    # package nobody has published to in a month can carry a `modified` stamped this
    # morning. Reading it would make the stalest possible package look freshly cut,
    # which is precisely the reading this script exists to prevent. No timestamp for
    # the published version itself is RED.
    times = packument.get("time") or {}
    age_days: float | None = None
    if published in times:
        try:
            age_days = (now - _parse_ts(times[published])).total_seconds() / 86400.0
        except ValueError as exc:
            problems.append(f"unparseable publish time for {published}: {exc}")
    else:
        problems.append(
            f"the packument carries no `time` entry for {published}, so its age "
            f"cannot be measured (and `time.modified` is not a substitute)"
        )

    distance: int | None = None
    try:
        pub = parse_version(published)
    except ValueError as exc:
        problems.append(f"cannot compare against the published version: {exc}")
        return Verdict(pkg.name, published, age_days, None, tuple(problems))

    distance = patch_distance(repo, pub)

    if pub > repo:
        # npm ahead of the checkout. Benign only if you are running this from an old
        # branch; on `main` it means a release was cut from something that never
        # merged, which is worth a human either way.
        problems.append(
            f"npm serves {published} but this checkout is {repo_version} -- the "
            f"registry is AHEAD of the tree the gate is measuring"
        )
    elif distance is None:
        problems.append(
            f"npm serves {published} while the tree is {repo_version}: a different "
            f"x.y line, so the patch distance is not a commit count. Treated as stale"
        )
    elif distance > 0:
        # Age counts only while the tree is ahead. Otherwise a deliberately quiet
        # month -- nothing merged, nothing to release -- would read as a failure, the
        # gate would be muted, and it would be gone when it was next needed.
        if age_days is not None and age_days > pkg.max_unpublished_age_days:
            problems.append(
                f"npm's `latest` is {published}, published {age_days:.1f} days ago "
                f"(budget {pkg.max_unpublished_age_days:g}d), while the tree is at "
                f"{repo_version} -- {distance} unreleased patch bumps behind"
            )
        if distance > pkg.max_version_distance:
            problems.append(
                f"npm's `latest` {published} is {distance} patch bumps behind "
                f"{repo_version} (budget {pkg.max_version_distance})"
            )

    # Partial publish: the launcher pins its optionalDependencies to its own exact
    # version, so a platform package on a different `latest` is an install failure
    # for everyone, no matter how young either publish is.
    if anchor_published is not None and published != anchor_published:
        problems.append(
            f"published {published} but the launcher publishes {anchor_published} -- "
            f"a partial publish; `npm install @perryts/perry` cannot resolve this "
            f"platform"
        )

    return Verdict(pkg.name, published, age_days, distance, tuple(problems))


def evaluate(
    packages: Sequence[Package],
    repo_version: str,
    now: _dt.datetime,
    fetch: Callable[[str], dict] = fetch_packument,
) -> list[Verdict]:
    """Evaluate every package, anchor first so the others can be compared to it."""
    ordered = sorted(packages, key=lambda p: (not p.anchor, p.name))
    packuments: dict[str, dict | RegistryError] = {}
    for pkg in ordered:
        try:
            packuments[pkg.name] = fetch(pkg.name)
        except RegistryError as exc:
            packuments[pkg.name] = exc

    anchor = next((p for p in ordered if p.anchor), None)
    anchor_doc = packuments.get(anchor.name) if anchor else None
    anchor_published = (
        (anchor_doc.get("dist-tags") or {}).get("latest")
        if isinstance(anchor_doc, dict)
        else None
    )

    verdicts = []
    for pkg in ordered:
        doc = packuments[pkg.name]
        if isinstance(doc, RegistryError):
            # See the module docstring: unreachable is RED. A skip that exits 0 is
            # the same silence that let #7491 run for a month.
            verdicts.append(Verdict(pkg.name, None, None, None, (str(doc),)))
            continue
        verdicts.append(
            evaluate_package(
                pkg,
                repo_version,
                doc,
                now,
                anchor_published=None if pkg.anchor else anchor_published,
            )
        )
    return verdicts


# --------------------------------------------------------------------------- output


def render(verdicts: Iterable[Verdict], repo_version: str) -> str:
    rows = list(verdicts)
    width = max((len(v.package) for v in rows), default=10)
    lines = [
        f"tree version: {repo_version}",
        "",
        f"{'package'.ljust(width)}  status   detail",
        f"{'-' * width}  -------  ------",
    ]
    for v in rows:
        lines.append(f"{v.package.ljust(width)}  {'STALE' if v.stale else 'ok':<7}  {v.detail}")
    return "\n".join(lines)


def issue_body(verdicts: Sequence[Verdict], repo_version: str) -> str:
    stale = [v for v in verdicts if v.stale]
    out = [
        f"The npm registry is not serving what `main` is at (`{repo_version}`).",
        "",
        "**What this costs when it is ignored** — [#7491]"
        "(https://github.com/PerryTS/perry/issues/7491): users installed a month-old "
        "`latest` and hit a linker error (`unresolved external symbol "
        "js_ext_http_client_request_is_handle`) that had been fixed on `main` for "
        "weeks. Every CI gate was green; they all measure `main`, and `main` is not "
        "what npm serves.",
        "",
        "| package | problem |",
        "|---|---|",
    ]
    for v in stale:
        for problem in v.problems:
            out.append(f"| `{v.package}` | {problem} |")
    out += [
        "",
        "Cut a release with `release-packages.yml` (see "
        "`docs/src/contributing/releasing.md`), or widen the budget in "
        "`scripts/npm_publish_freshness.json` and say why in the same commit.",
        "",
        "_Maintained automatically by `npm-publish-freshness.yml`. Updated in place, "
        "never duplicated, and closed once the registry has caught up._",
    ]
    return "\n".join(out)


# --------------------------------------------------------------------------- sticky issue


def _gh(args: list[str]) -> str:
    proc = subprocess.run(["gh", *args], capture_output=True, text=True)
    if proc.returncode != 0:
        raise RuntimeError(f"gh {' '.join(args)} failed: {proc.stderr.strip()}")
    return proc.stdout


def sync_issue(repo: str, verdicts: Sequence[Verdict], repo_version: str) -> None:
    """Open, update, or close the single sticky alert issue. Never duplicates."""
    found = json.loads(
        _gh(
            [
                "issue",
                "list",
                "--repo",
                repo,
                "--state",
                "all",
                "--search",
                f'"{ISSUE_MARKER}" in:title',
                "--json",
                "number,title,state",
                "--limit",
                "10",
            ]
        )
    )
    existing = next((i for i in found if ISSUE_MARKER in i["title"]), None)
    stale = [v for v in verdicts if v.stale]

    if not stale:
        if existing is not None and existing["state"] == "OPEN":
            _gh(
                [
                    "issue",
                    "close",
                    str(existing["number"]),
                    "--repo",
                    repo,
                    "--comment",
                    "npm is serving a current build again; closing automatically.",
                ]
            )
            print(f"closed sticky issue #{existing['number']} (registry is current)")
        return

    title = f"{ISSUE_MARKER}: {len(stale)} package(s) behind `{repo_version}`"
    body = issue_body(verdicts, repo_version)
    if existing is None:
        url = _gh(["issue", "create", "--repo", repo, "--title", title, "--body", body]).strip()
        print(f"opened sticky issue {url}")
    else:
        if existing["state"] != "OPEN":
            _gh(["issue", "reopen", str(existing["number"]), "--repo", repo])
        _gh(
            [
                "issue",
                "edit",
                str(existing["number"]),
                "--repo",
                repo,
                "--title",
                title,
                "--body",
                body,
            ]
        )
        print(f"updated sticky issue #{existing['number']}")


# --------------------------------------------------------------------------- self-test


def self_test() -> int:
    """Plant every failure shape one at a time and assert the verdict.

    CLAUDE.md: a gate must assert its subject was live, not merely that nothing threw.
    Running the happy path and printing OK would prove only that the code parses.
    """
    now = _dt.datetime(2026, 8, 13, 12, 0, tzinfo=_dt.timezone.utc)
    repo_version = "0.5.1510"

    def stamp(days_ago: float) -> str:
        return (now - _dt.timedelta(days=days_ago)).strftime("%Y-%m-%dT%H:%M:%S.000Z")

    def pack(version: str, days_ago: float, **kw) -> dict:
        doc: dict = {
            "dist-tags": {"latest": version},
            "versions": {version: {"name": "x", "version": version}},
            "time": {"modified": stamp(kw.get("modified_days_ago", days_ago))},
        }
        if not kw.get("omit_time"):
            doc["time"][version] = stamp(days_ago)
        if kw.get("no_latest"):
            doc["dist-tags"] = {}
        if kw.get("unlisted"):
            doc["versions"] = {}
        return doc

    budget = Package("pkg", 14, 500, False, "self-test")

    cases: list[tuple[str, dict, bool, str | None]] = [
        # (label, packument, want_stale, substring the message must contain)
        (
            "published == tree, published long ago: nothing unreleased, so not stale",
            pack("0.5.1510", 90),
            False,
            None,
        ),
        (
            "the #7491 shape: a month-old latest with 290 patches behind it",
            pack("0.5.1220", 32),
            True,
            "0.5.1220",
        ),
        (
            "age alone: only 3 unreleased patches, but 20 days unpublished",
            pack("0.5.1507", 20),
            True,
            "20.0 days ago",
        ),
        (
            "distance alone: published 2 days ago but 900 patches back",
            pack("0.5.610", 2),
            True,
            "900 patch bumps behind",
        ),
        (
            "THE TRAP: `time.modified` is today, the version itself is 40 days old",
            pack("0.5.1505", 40, modified_days_ago=0),
            True,
            "40.0 days ago",
        ),
        (
            "no `latest` dist-tag at all",
            pack("0.5.1510", 1, no_latest=True),
            True,
            "no `latest` dist-tag",
        ),
        (
            "dist-tag points at a version the packument does not list",
            pack("0.5.1400", 1, unlisted=True),
            True,
            "did not complete",
        ),
        (
            "no `time` entry for the published version",
            pack("0.5.1400", 1, omit_time=True),
            True,
            "no `time` entry",
        ),
        (
            "a timezone-naive publish timestamp is rejected cleanly",
            {
                **pack("0.5.1400", 1),
                "time": {
                    "modified": stamp(1),
                    "0.5.1400": "2026-08-12T12:00:00",
                },
            },
            True,
            "no UTC offset",
        ),
        (
            "a prerelease on `latest` is not silently ordered",
            pack("0.6.0-beta.1", 1),
            True,
            "not a plain x.y.z",
        ),
        (
            "npm ahead of the checkout",
            pack("0.5.1600", 1),
            True,
            "AHEAD",
        ),
        (
            "a different x.y line is not a commit count",
            pack("0.4.900", 1),
            True,
            "different x.y line",
        ),
        (
            "exactly at both budgets is not yet stale",
            pack("0.5.1010", 14),
            False,
            None,
        ),
    ]

    failures: list[str] = []
    for label, doc, want_stale, needle in cases:
        v = evaluate_package(budget, repo_version, doc, now)
        if v.stale != want_stale:
            failures.append(f"{label}: expected stale={want_stale}, got {v.problems}")
        elif needle and not any(needle in p for p in v.problems):
            failures.append(f"{label}: no message contained {needle!r}; got {v.problems}")

    # A partial publish: both packages are individually inside every budget, and the
    # install is still broken. Only the cross-package invariant catches this.
    anchor = Package("@perryts/perry", 14, 500, True, "")
    platform = Package("@perryts/perry-darwin-arm64", 14, 500, False, "")

    def fake_fetch(name: str) -> dict:
        return {
            "@perryts/perry": pack("0.5.1510", 1),
            "@perryts/perry-darwin-arm64": pack("0.5.1505", 1),
        }[name]

    partial = {v.package: v for v in evaluate([anchor, platform], repo_version, now, fake_fetch)}
    if partial["@perryts/perry"].stale:
        failures.append(f"partial publish: the anchor itself should be ok, got {partial['@perryts/perry'].problems}")
    if not partial["@perryts/perry-darwin-arm64"].stale:
        failures.append("partial publish: a platform package on a different latest must be stale")
    elif not any("partial publish" in p for p in partial["@perryts/perry-darwin-arm64"].problems):
        failures.append(f"partial publish: wrong message: {partial['@perryts/perry-darwin-arm64'].problems}")

    # An unreachable registry must be RED, never a quiet pass. This is the assertion
    # that keeps the module docstring's decision from being reverted by accident.
    def dead_fetch(name: str) -> dict:
        raise RegistryError("Temporary failure in name resolution")

    dead = evaluate([anchor], repo_version, now, dead_fetch)
    # Check the COUNT before indexing. The failure being guarded against here is
    # a `continue` that drops the unreachable package instead of recording a
    # verdict for it, which leaves this list empty -- and a bare `dead[0]` turns
    # that into an IndexError traceback rather than the sentence explaining what
    # broke. Both exit non-zero, so the gate holds either way; the difference is
    # whether the next reader has to reverse-engineer the mutation.
    if not dead:
        failures.append(
            "an unreachable registry produced NO verdict at all -- it was dropped "
            "rather than reported, which is the silence this check exists to end"
        )
    elif not dead[0].stale:
        failures.append("an unreachable registry passed -- a skip is reading as a pass")
    if _exit_code(dead) == 0:
        failures.append("exit code was 0 for an unreachable registry")

    # Coverage drift: a platform package published from npm/ but absent from the
    # manifest must fail, or the gate can shrink without going red.
    if not coverage_problems({"a", "b"}, {"a"}):
        failures.append("coverage_problems missed a package that npm/ publishes")
    if not coverage_problems({"a"}, {"a", "b"}):
        failures.append("coverage_problems missed a manifest entry with no npm/ dir")
    if coverage_problems({"a", "b"}, {"a", "b"}):
        failures.append("coverage_problems flagged an exactly-matching set")

    # A nameless template must fail closed. Silently skipping it would let the
    # set of published packages grow while the manifest check still passed.
    with tempfile.TemporaryDirectory() as tmp:
        package_dir = Path(tmp) / "nameless"
        package_dir.mkdir()
        (package_dir / "package.json.tmpl").write_text('{"version": "0.0.0"}\n')
        try:
            npm_package_names(Path(tmp))
        except SystemExit as exc:
            if "declares no literal `name`" not in str(exc):
                failures.append(f"nameless template produced the wrong failure: {exc}")
        else:
            failures.append("nameless package.json.tmpl was silently skipped")

    # The shipped manifest must actually cover the shipped packages, and parse.
    real = load_packages()
    real_problems = coverage_problems(npm_package_names(), {p.name for p in real})
    for problem in real_problems:
        failures.append(f"shipped manifest: {problem}")

    # A stale verdict must PRINT as stale and EXIT non-zero; a printed table that
    # says nothing, or a red verdict with exit 0, is a gate that cannot fail.
    stale_verdicts = [evaluate_package(budget, repo_version, pack("0.5.1220", 32), now)]
    if "STALE" not in render(stale_verdicts, repo_version):
        failures.append("render() produced no STALE marker for a stale set")
    if _exit_code(stale_verdicts) == 0:
        failures.append("exit code was 0 despite a stale package")
    if _exit_code([evaluate_package(budget, repo_version, pack("0.5.1510", 90), now)]) != 0:
        failures.append("exit code was non-zero for an up-to-date package")

    # A later stale episode must reopen the same closed sticky issue, not create
    # a new issue every release cycle while claiming to maintain one forever.
    issue_calls: list[list[str]] = []

    def fake_gh(args: list[str]) -> str:
        issue_calls.append(args)
        if args[:2] == ["issue", "list"]:
            return json.dumps(
                [
                    {
                        "number": 7491,
                        "title": f"{ISSUE_MARKER}: prior episode",
                        "state": "CLOSED",
                    }
                ]
            )
        return ""

    original_gh = globals()["_gh"]
    globals()["_gh"] = fake_gh
    try:
        sync_issue("PerryTS/perry", stale_verdicts, repo_version)
    finally:
        globals()["_gh"] = original_gh
    issue_verbs = [args[1] for args in issue_calls if args and args[0] == "issue"]
    if issue_verbs != ["list", "reopen", "edit"]:
        failures.append(
            "closed sticky issue was not reused; expected list/reopen/edit, got "
            f"{issue_verbs}"
        )

    if failures:
        print("SELF-TEST FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print(
        f"self-test OK: {len(cases)} planted packuments, partial-publish, "
        f"registry-unreachable and coverage-drift shapes, all verdicts as expected"
    )
    return 0


def _exit_code(verdicts: Sequence[Verdict]) -> int:
    return 1 if any(v.stale for v in verdicts) else 0


# --------------------------------------------------------------------------- main


def check_manifest(repo_version: str) -> int:
    """The offline half: manifest parses, covers npm/, and the tree version reads."""
    packages = load_packages()
    problems = coverage_problems(npm_package_names(), {p.name for p in packages})
    for problem in problems:
        print(f"::error::{problem}")
    if problems:
        return 1
    print(
        f"manifest ok: tree is {repo_version}, watching {len(packages)} package(s) "
        f"({', '.join(p.name for p in packages)})"
    )
    return 0


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    ap.add_argument("--self-test", action="store_true", help="prove the checker can fail")
    ap.add_argument(
        "--check-manifest",
        action="store_true",
        help="offline: manifest parses and covers every npm/ package",
    )
    ap.add_argument("--dry-run", action="store_true", help="query the registry, touch no issues")
    ap.add_argument("--package", action="append", default=[], help="scope to one package (diagnostic)")
    ap.add_argument(
        "--packument",
        help="replay a saved registry response instead of fetching: either one "
        "packument, or a {package: packument} map",
    )
    ap.add_argument("--repo-version", help="compare against this version instead of Cargo.toml")
    ap.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY", "PerryTS/perry"))
    args = ap.parse_args(argv)

    if args.self_test:
        return self_test()

    repo_version = args.repo_version or workspace_version()

    if args.check_manifest:
        return check_manifest(repo_version)

    packages = load_packages()
    if args.package:
        wanted = set(args.package)
        unknown = wanted - {p.name for p in packages}
        if unknown:
            raise SystemExit(f"--package names nothing in {MANIFEST.name}: {sorted(unknown)}")
        packages = [p for p in packages if p.name in wanted]

    fetch: Callable[[str], dict] = fetch_packument
    if args.packument:
        saved = json.loads(Path(args.packument).read_text())
        # A file holding one packument is used for every selected package; a map is
        # keyed by package name. Both shapes are for replaying an incident offline.
        def fetch(name: str, _saved=saved) -> dict:
            if "dist-tags" in _saved:
                return _saved
            if name not in _saved:
                raise RegistryError(f"no saved packument for {name}")
            return _saved[name]

    verdicts = evaluate(packages, repo_version, _dt.datetime.now(_dt.timezone.utc), fetch)
    if not verdicts:
        # `_exit_code([])` is 0. Nothing should be able to reach here with an empty
        # set, and a gate that reports success having compared nothing is the whole
        # hazard, so say so instead of finding out later.
        raise SystemExit("no packages were evaluated; refusing to report success")

    table = render(verdicts, repo_version)
    print(table)

    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a") as fh:
            fh.write(f"## npm publish freshness (tree {repo_version})\n\n```\n{table}\n```\n")

    stale = [v for v in verdicts if v.stale]
    for v in stale:
        for problem in v.problems:
            print(f"::error::{v.package}: {problem}")

    # A scoped or replayed run is a diagnostic, not a verdict on the whole set: it
    # must never be able to close the alert for packages it did not look at.
    scoped = bool(args.package or args.packument)
    if not args.dry_run and not scoped:
        try:
            sync_issue(args.repo, verdicts, repo_version)
        except RuntimeError as exc:
            # Failing to file the alert must not mask the alert itself.
            print(f"::warning::could not sync the sticky issue: {exc}", file=sys.stderr)

    if stale:
        print(
            f"\n{len(stale)} of {len(verdicts)} watched package(s) are behind "
            f"{repo_version}. Cut a release (docs/src/contributing/releasing.md) or "
            f"argue the budget in {MANIFEST.name}.",
            file=sys.stderr,
        )
    return _exit_code(verdicts)


if __name__ == "__main__":
    sys.exit(main())
