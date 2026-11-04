#!/usr/bin/env python3
"""Resolve stable GC witness ids to their unique runnable TypeScript source."""

from __future__ import annotations

import argparse
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
            return f"manifest lists {self.case_id} but test-files/{self.case_id}.ts does not exist"
        names = ", ".join(sorted(path.relative_to(root).as_posix() for path in self.paths))
        return f"ambiguous GC witness source {self.case_id!r}: {names}"


class SourceResolutionError(ValueError):
    def __init__(self, issues: tuple[SourceIssue, ...], root: Path) -> None:
        self.issues = issues
        super().__init__("\n".join(issue.diagnostic(root) for issue in issues))


def source_patterns() -> tuple[str, ...]:
    return ("test-files/*.ts",) + tuple(
        f"test-parity/{root}/**/*.ts" for root in parity.SUBJECT_ROOTS
    )


def resolve_sources(
    root: Path,
    ids: Iterable[str],
    discover: Callable[[str], Iterable[str | Path]] | None = None,
) -> list[tuple[str, Path]]:
    """Resolve each requested basename once, preserving request order/multiplicity.

    `discover` is the filesystem seam used by the registration checker's virtual
    Tree. Returned paths stay lexical and repo-relative, unlike Path.resolve's
    /tmp -> /private/tmp canonicalisation on macOS.
    """
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


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd())
    parser.add_argument("ids", nargs="*")
    args = parser.parse_args()
    try:
        for case_id, path in resolve_sources(args.root, args.ids):
            print(f"{case_id}\t{path.relative_to(args.root).as_posix()}")
    except SourceResolutionError as exc:
        print(exc.issues[0].diagnostic(args.root), file=sys.stderr)
        return 3
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
