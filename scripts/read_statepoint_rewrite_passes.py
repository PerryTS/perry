#!/usr/bin/env python3
"""Read production's statepoint pass pipeline from its unique Rust constant."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


REPO = Path(__file__).resolve().parent.parent
CODEGEN_SRC = REPO / "crates" / "perry-codegen" / "src"
DECLARATION = re.compile(
    r"\bpub\(crate\)\s+const\s+STATEPOINT_REWRITE_PASSES\s*:\s*&str\s*=\s*"
    r'"([^"\\]*)"\s*;',
    re.MULTILINE | re.DOTALL,
)


def extract(text: str) -> str | None:
    match = DECLARATION.search(text)
    return match.group(1) if match else None


def find_declaration(root: Path = CODEGEN_SRC) -> tuple[Path, str]:
    matches = []
    for path in sorted(root.rglob("*.rs")):
        value = extract(path.read_text(encoding="utf-8"))
        if value is not None:
            matches.append((path, value))

    if len(matches) != 1:
        locations = ", ".join(str(path) for path, _value in matches) or "none"
        raise ValueError(
            "expected exactly one literal STATEPOINT_REWRITE_PASSES declaration "
            f"under {root}, found {len(matches)} ({locations})"
        )
    path, value = matches[0]
    if not value or "rewrite-statepoints-for-gc" not in value:
        raise ValueError(
            f"{path} does not contain a usable statepoint rewrite pipeline"
        )
    return path, value


def self_test() -> int:
    failures = []
    expected = "always-inline,function(mem2reg),rewrite-statepoints-for-gc"
    one_line = (
        'pub(crate) const STATEPOINT_REWRITE_PASSES: &str = "' + expected + '";'
    )
    wrapped = (
        "pub(crate) const STATEPOINT_REWRITE_PASSES: &str =\n"
        f'    "{expected}";\n'
    )
    for label, source in (("one-line", one_line), ("rustfmt-wrapped", wrapped)):
        if extract(source) != expected:
            failures.append(f"{label} declaration was not read")
    if extract(one_line.replace("STATEPOINT_REWRITE_PASSES", "OTHER_PASSES")) is not None:
        failures.append("a differently named constant was accepted")

    try:
        path, value = find_declaration()
        if not path.is_relative_to(CODEGEN_SRC):
            failures.append("the repository declaration escaped perry-codegen/src")
        if value != extract(path.read_text(encoding="utf-8")):
            failures.append("the repository scan disagrees with the source parser")
    except (OSError, ValueError) as error:
        failures.append(str(error))

    for failure in failures:
        print(f"statepoint-pass reader self-test FAILED: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("statepoint-pass reader self-test: OK")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    try:
        path, value = find_declaration()
    except (OSError, ValueError) as error:
        print(f"statepoint-pass reader: {error}", file=sys.stderr)
        return 2

    if args.check:
        print(
            "statepoint-pass source OK: "
            f"{path.relative_to(REPO)} -> {value}"
        )
    else:
        print(value)
    return 0


if __name__ == "__main__":
    sys.exit(main())
