#!/usr/bin/env python3
"""Assert the shape of the changelog archive and keep the pending queue bounded.

Layout this enforces (see changelog.d/README.md):

    changelog.d/        the pending queue: fragments no release has shipped yet
    changelog/vX.Y.Z/   exactly the entries that shipped in vX.Y.Z, plus the
                        manifest.json scripts/cut_release_notes.sh writes

WHY A GATE AND NOT A DOCUMENTED STEP
------------------------------------
The fold is the one release step whose only observable result is "files moved".
It lived in changelog.d/README.md as a manual post-release task and never ran
once: 0 `chore(release): fold changesets` commits, 4,211 fragments, 5.7 MB of
text, and every release body truncated at GitHub's 125,000-character limit
because the notes carried months of relaunched history. A cleanup nobody
asserts is a cleanup that does not happen.

WHAT THIS DELIBERATELY DOES NOT CHECK
-------------------------------------
That every archive folder has a matching git tag. CI checks out with
`--no-tags --depth 1`, so tags are absent on a PR and the assertion could not
pass. The folder name and the manifest version are written from the same
argument by the fold script, so they are self-consistent by construction;
scripts/cut_release_notes.sh is the only writer.

WHAT THE FRAGMENT CAP IS FOR
----------------------------
It is a smoke alarm for "the fold stopped happening", not a style limit. One
release cycle's worth of Perry fragments is large: v0.5.1520 shipped 1,509
entries, and the 0.5.1664 cycle is ~2,700 at roughly 28 PRs/day between tags.
The state that produced the archive in the first place was 4,211 = two missed
cycles. So the cap sits at 6,000 — it does not fire on a long-but-normal cycle,
and it does fire on the multi-release drift it exists to catch. A fold resets
the queue. When it trips, the message names the command that fixes it.
"""
from __future__ import annotations

import argparse
import json
import re
import shutil
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]

VERSION_RE = re.compile(r"^v\d+\.\d+\.\d+$")
# README.md is documentation; HIGHLIGHTS.md is the hand-written summary printed
# above a release's entries. Neither is an entry.
NOT_ENTRIES = frozenset({"README.md", "HIGHLIGHTS.md"})
DEFAULT_MAX_PENDING = 6000
SOURCES = frozenset({"tag", "pending"})


def entry_files(directory: Path) -> list[Path]:
    return sorted(p for p in directory.glob("*.md") if p.name not in NOT_ENTRIES)


def check(root: Path, max_pending: int = DEFAULT_MAX_PENDING) -> list[str]:
    pending_dir = root / "changelog.d"
    archive_root = root / "changelog"
    if not pending_dir.is_dir():
        return [f"{pending_dir} is missing: the pending queue must exist"]
    if not archive_root.is_dir():
        return [
            f"{archive_root} is missing: released fragments belong in "
            f"changelog/vX.Y.Z/ (see changelog.d/README.md)"
        ]

    errors: list[str] = []
    pending_entries = entry_files(pending_dir)
    pending_names = {p.name for p in pending_entries}

    for child in sorted(archive_root.iterdir()):
        if not child.is_dir():
            errors.append(
                f"{child}: stray file — an archived release is a directory, "
                f"changelog/vX.Y.Z/"
            )
            continue
        if not VERSION_RE.match(child.name):
            errors.append(f"{child.name}: directory name is not a version (vX.Y.Z)")
            continue
        entries = entry_files(child)
        if not entries:
            errors.append(
                f"{child.name}: no fragments — an archive is the set that "
                f"shipped, so it is never empty"
            )
            continue
        manifest_error = _manifest_errors(child, len(entries))
        errors.extend(manifest_error)
        duplicated = pending_names & {p.name for p in entries}
        if duplicated:
            listed = ", ".join(sorted(duplicated)[:5])
            errors.append(
                f"{child.name}: {len(duplicated)} fragment(s) also pending in "
                f"changelog.d/ ({listed}) — a released fragment is archived once, "
                f"never in both places"
            )

    if len(pending_entries) > max_pending:
        errors.append(
            f"changelog.d/ holds {len(pending_entries)} fragments, over the "
            f"{max_pending} cap: the fold stopped happening. Run "
            f"./scripts/cut_release_notes.sh --fold vX.Y.Z on the release branch."
        )
    return errors


def _manifest_errors(archive: Path, entries: int) -> list[str]:
    path = archive / "manifest.json"
    if not path.is_file():
        return [f"{archive.name}: manifest.json is missing"]
    try:
        manifest = json.loads(path.read_text())
    except json.JSONDecodeError as exc:
        return [f"{archive.name}: manifest.json is not valid JSON ({exc})"]
    if not isinstance(manifest, dict):
        return [f"{archive.name}: manifest.json is not a JSON object"]
    errors = []
    if manifest.get("version") != archive.name:
        errors.append(
            f"{archive.name}: manifest version is {manifest.get('version')!r}, "
            f"not {archive.name!r}"
        )
    if manifest.get("entries") != entries:
        errors.append(
            f"{archive.name}: manifest entries is {manifest.get('entries')!r}, "
            f"but {entries} fragment(s) are present"
        )
    if manifest.get("source") not in SOURCES:
        errors.append(
            f"{archive.name}: manifest source is {manifest.get('source')!r}, "
            f"not one of {sorted(SOURCES)}"
        )
    return errors


def report(root: Path, errors: list[str], max_pending: int) -> int:
    if errors:
        for error in errors:
            print(f"ERROR: {error}", file=sys.stderr)
        print(f"changelog archive: {len(errors)} problem(s)", file=sys.stderr)
        return 1
    archives = [p for p in (root / "changelog").iterdir() if p.is_dir()]
    pending = len(entry_files(root / "changelog.d"))
    print(
        f"changelog archive: {len(archives)} archived release(s), "
        f"{pending} pending fragment(s) (cap {max_pending})"
    )
    return 0


def _build(
    base: Path,
    *,
    pending: tuple[str, ...] = ("100-a.md", "9-b.md"),
    archive_entries: tuple[str, ...] = ("100-a.md", "9-b.md", "unkeyed-c.md"),
    archive_name: str | None = "v1.0.0",
    manifest: bool = True,
    manifest_patch: dict | None = None,
) -> Path:
    pending_dir = base / "changelog.d"
    pending_dir.mkdir(parents=True)
    (pending_dir / "README.md").write_text("# fragments\n")
    for name in pending:
        (pending_dir / name).write_text("entry\n")
    archive_root = base / "changelog"
    archive_root.mkdir()
    if archive_name is not None:
        archive = archive_root / archive_name
        archive.mkdir()
        for name in archive_entries:
            (archive / name).write_text("entry\n")
        if manifest:
            data = {
                "version": archive_name,
                "source": "tag",
                "entries": len(archive_entries),
                "highlights": False,
            }
            data.update(manifest_patch or {})
            (archive / "manifest.json").write_text(json.dumps(data))
    return base


def self_test() -> int:
    failures: list[str] = []

    def expect(label: str, errors: list[str], needle: str | None = None) -> None:
        if needle is None:
            if not errors:
                print(f"  ok   {label}")
            else:
                failures.append(f"{label}: expected no error, got {errors}")
            return
        if any(needle in error for error in errors):
            print(f"  ok   {label}")
        else:
            failures.append(f"{label}: no error mentioned {needle!r}, got {errors}")

    print("check_changelog_archive.py --self-test")
    with tempfile.TemporaryDirectory() as tmp:
        tmp_path = Path(tmp)

        good = _build(tmp_path / "good", pending=("999-fresh.md",))
        expect("well-formed tree", check(good))

        missing_queue = _build(tmp_path / "no-queue")
        shutil.rmtree(missing_queue / "changelog.d")
        expect("missing pending queue", check(missing_queue), "is missing")

        missing_archive = _build(tmp_path / "no-archive", archive_name=None)
        shutil.rmtree(missing_archive / "changelog")
        expect("missing archive root", check(missing_archive), "changelog")

        no_manifest = _build(tmp_path / "no-manifest", manifest=False)
        expect("missing manifest", check(no_manifest), "manifest.json is missing")

        bad_version = _build(
            tmp_path / "bad-version", manifest_patch={"version": "v9.9.9"}
        )
        expect("manifest version mismatch", check(bad_version), "manifest version is")

        bad_count = _build(tmp_path / "bad-count", manifest_patch={"entries": 7})
        expect("manifest entry count mismatch", check(bad_count), "manifest entries is")

        bad_source = _build(
            tmp_path / "bad-source", manifest_patch={"source": "manual"}
        )
        expect("manifest source invalid", check(bad_source), "manifest source is")

        empty_archive = _build(tmp_path / "empty", archive_entries=())
        expect("empty archive", check(empty_archive), "no fragments")

        bad_name = _build(tmp_path / "bad-name", archive_name="v1.0")
        expect("non-version archive name", check(bad_name), "not a version")

        stray = _build(tmp_path / "stray", archive_name=None)
        (stray / "changelog" / "v2.0.0.md").write_text("flat archive\n")
        expect("flat archive file", check(stray), "stray file")

        double = _build(tmp_path / "double", pending=("100-a.md",))
        expect("fragment both archived and pending", check(double), "also pending")

        capped = _build(tmp_path / "cap", pending=("1-a.md", "2-b.md", "3-c.md"))
        expect("pending queue over the cap", check(capped, max_pending=2), "over the 2 cap")

    print()
    if failures:
        for failure in failures:
            print(f"ERROR: {failure}", file=sys.stderr)
        print(f"self-test FAILED: {len(failures)} assertion(s)", file=sys.stderr)
        return 1
    print("self-test passed")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument(
        "--max-pending",
        type=int,
        default=DEFAULT_MAX_PENDING,
        help=f"pending fragments allowed in changelog.d/ (default {DEFAULT_MAX_PENDING})",
    )
    parser.add_argument("--root", type=Path, default=ROOT, help=argparse.SUPPRESS)
    args = parser.parse_args()
    if args.self_test:
        return self_test()
    return report(args.root, check(args.root, args.max_pending), args.max_pending)


if __name__ == "__main__":
    sys.exit(main())
