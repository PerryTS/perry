#!/usr/bin/env python3
import re
import sys
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[1]
EXT_SRC_ROOTS = sorted((REPO_ROOT / "crates").glob("perry-ext-*/src"))
ANONYMOUS_MUTABLE_REGISTRATION = re.compile(
    r"(?<![A-Za-z0-9_])gc_register_mutable_root_scanner\s*\("
)
NAMED_MUTABLE_REGISTRATION = re.compile(
    r"gc_register_mutable_root_scanner_named\s*\(\s*\"([^\"]+)\""
)


def main() -> int:
    anonymous_calls = []
    named_calls = []

    for src_root in EXT_SRC_ROOTS:
        crate_name = src_root.parent.name
        for path in sorted(src_root.rglob("*.rs")):
            text = path.read_text(encoding="utf-8")
            if ANONYMOUS_MUTABLE_REGISTRATION.search(text):
                anonymous_calls.append(path.relative_to(REPO_ROOT).as_posix())
            for match in NAMED_MUTABLE_REGISTRATION.finditer(text):
                named_calls.append(
                    (
                        crate_name,
                        path.relative_to(REPO_ROOT).as_posix(),
                        match.group(1),
                    )
                )

    if anonymous_calls:
        print(
            "perry-ext mutable GC scanners must use "
            f"gc_register_mutable_root_scanner_named: {anonymous_calls}",
            file=sys.stderr,
        )
        return 1

    if not named_calls:
        print("expected at least one named perry-ext GC scanner", file=sys.stderr)
        return 1

    mismatched_sources = [
        (path, source, crate_name)
        for crate_name, path, source in named_calls
        if source != crate_name
    ]
    if mismatched_sources:
        print(
            "perry-ext GC scanner source must match the crate name: "
            f"{mismatched_sources}",
            file=sys.stderr,
        )
        return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
