#!/usr/bin/env python3
"""Resolve GC witness ids or basename patterns across program subject roots."""

from __future__ import annotations

import argparse
from fnmatch import fnmatchcase
from pathlib import Path
import sys
from typing import Callable, Iterable

SCRIPTS = Path(__file__).parent
sys.path.insert(0, str(SCRIPTS))
import parity  # noqa: E402


class SourceIssue:
    def __init__(self, case_id: str, kind: str, paths: tuple[Path, ...] = ()) -> None:
        self.case_id = case_id
        self.kind = kind
        self.paths = paths

    def diagnostic(self, root: Path) -> str:
        if self.kind == "unknown":
            return f"manifest lists {self.case_id} but tests/fixtures/programs/{self.case_id}.ts does not exist"
        if self.kind == "unmatched_pattern":
            return (f"::error::pattern '{self.case_id}' matched nothing; it is stale. "
                    "Remove it or fix it.")
        names = ", ".join(sorted(path.relative_to(root).as_posix() for path in self.paths))
        return f"ambiguous GC witness source {self.case_id!r}: {names}"


class SourceResolutionError(ValueError):
    def __init__(self, issues: tuple[SourceIssue, ...], root: Path) -> None:
        self.issues = issues
        super().__init__("\n".join(issue.diagnostic(root) for issue in issues))


def source_patterns() -> tuple[str, ...]:
    return ("tests/fixtures/programs/*.ts",) + tuple(
        f"tests/parity/{root}/**/*.ts" for root in parity.SUBJECT_ROOTS
    )


def _candidate_sources(
    root: Path,
    discover: Callable[[str], Iterable[str | Path]] | None = None,
) -> list[Path]:
    root = Path(root)
    if discover is None:
        discover = lambda pattern: root.glob(pattern)
    candidates: list[Path] = []
    for pattern in source_patterns():
        for found in discover(pattern):
            path = Path(found)
            if not path.is_absolute():
                path = root / path
            if path.suffix == ".ts" and parity.is_case(root, path):
                candidates.append(path)

    return candidates


def _resolve_ids(root: Path, ids: Iterable[str], candidates: list[Path]) -> list[tuple[str, Path]]:
    result: list[tuple[str, Path]] = []
    errors: list[SourceIssue] = []
    for case_id in ids:
        matches = [path for path in candidates if path.stem == case_id]
        if not matches:
            errors.append(SourceIssue(case_id, "unknown"))
        elif len(matches) != 1:
            errors.append(SourceIssue(case_id, "ambiguous", tuple(matches)))
        else:
            result.append((case_id, matches[0]))
    if errors:
        raise SourceResolutionError(tuple(errors), root)
    return result


def resolve_sources(
    root: Path,
    ids: Iterable[str],
    discover: Callable[[str], Iterable[str | Path]] | None = None,
) -> list[tuple[str, Path]]:
    """Resolve requested basenames, preserving order and repeated requests.

    `discover` supports the registration checker's virtual Tree. Paths stay
    lexical, including macOS /tmp rather than /private/tmp canonicalisation.
    """
    root = Path(root)
    return _resolve_ids(root, ids, _candidate_sources(root, discover))


def resolve_patterns(
    root: Path,
    patterns: Iterable[str],
    discover: Callable[[str], Iterable[str | Path]] | None = None,
    *,
    require_each: bool = True,
) -> list[tuple[str, Path]]:
    """Resolve basename globs in pattern order, retaining overlapping matches."""
    root = Path(root)
    candidates = _candidate_sources(root, discover)
    ids = []
    for pattern in patterns:
        matches = sorted((path for path in candidates if fnmatchcase(path.name, pattern)),
                         key=lambda path: (path.name, path.as_posix()))
        if not matches and require_each:
            raise SourceResolutionError((SourceIssue(pattern, "unmatched_pattern"),), root)
        ids.extend(path.stem for path in matches)
    return _resolve_ids(root, ids, candidates)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("ids", nargs="*")
    parser.add_argument("--pattern", action="append", default=[], help="basename glob; repeat in corpus order")
    parser.add_argument("--allow-empty-patterns", action="store_true", help="allow empty optional registration globs")
    args = parser.parse_args()
    if args.pattern and args.ids:
        parser.error("choose witness ids or --pattern, not both")
    if args.allow_empty_patterns and not args.pattern:
        parser.error("--allow-empty-patterns requires --pattern")
    try:
        sources = (resolve_patterns(args.root, args.pattern, require_each=not args.allow_empty_patterns)
                   if args.pattern else resolve_sources(args.root, args.ids))
        for case_id, path in sources:
            print(f"{case_id}\t{path.relative_to(args.root).as_posix()}")
    except SourceResolutionError as exc:
        print(exc.issues[0].diagnostic(args.root), file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
