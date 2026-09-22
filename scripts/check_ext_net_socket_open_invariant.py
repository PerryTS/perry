#!/usr/bin/env python3
"""Enforce perry-ext-net's ``is_open`` / ``has_opened`` invariant (#11056).

``SocketState.has_opened`` is monotonic history: every real transition to
``is_open = true`` must set ``has_opened = true`` on the same receiver in the
same Rust block.  ``socket.pending`` depends on that history, so missing the
second assignment leaves an already-connected socket looking pending forever.

There is one intentional exception today.  The already-aborted TLS path marks
a synthetic socket live long enough to deliver deferred ``error`` / ``close``
events, but that socket never opened.  Exceptions live in
``scripts/ext_net_socket_open_exceptions.json`` and are identity-pinned by
path, function, and receiver.  A new unpaired transition fails, and an
exception that stops matching an unpaired transition also fails so fixes must
delete their own exemption.

This is a deliberately small Rust source audit rather than a parser.  It masks
comments and literals, balances braces, enumerates every ``.is_open = true``
suffix, and rejects receiver syntax it cannot prove.  ``MIN_ASSIGNMENTS`` and
the self-test keep a broken matcher from reporting an empty green census.

Usage:
    python3 scripts/check_ext_net_socket_open_invariant.py
    python3 scripts/check_ext_net_socket_open_invariant.py --self-test
    python3 scripts/check_ext_net_socket_open_invariant.py --list
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Iterable


ROOT = Path(__file__).resolve().parent.parent
SOURCE_ROOT = ROOT / "crates" / "perry-ext-net" / "src"
EXCEPTIONS_PATH = ROOT / "scripts" / "ext_net_socket_open_exceptions.json"
MIN_ASSIGNMENTS = 4

OPEN_ASSIGNMENT = re.compile(r"\.\s*is_open\s*=\s*true\b")
SIMPLE_RECEIVER = re.compile(
    r"([A-Za-z_][A-Za-z0-9_]*(?:\s*\.\s*[A-Za-z_][A-Za-z0-9_]*)*)\s*$"
)
FUNCTION_NAME = re.compile(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)\b")
CHAR_LITERAL = re.compile(
    r"'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}|.)|[^'\\\n])'"
)
RAW_STRING_START = re.compile(r'(?:b|c)?r(#{0,255})"')


@dataclass(frozen=True)
class Site:
    path: str
    function: str
    receiver: str | None
    line: int
    paired: bool

    @property
    def key(self) -> tuple[str, str, str] | None:
        if self.receiver is None:
            return None
        return (self.path, self.function, self.receiver)


@dataclass(frozen=True)
class ExceptionEntry:
    name: str
    path: str
    function: str
    receiver: str
    reason: str

    @property
    def key(self) -> tuple[str, str, str]:
        return (self.path, self.function, self.receiver)


def _blank(chars: list[str], start: int, end: int) -> None:
    """Replace a non-code range with spaces while preserving newlines."""

    for index in range(start, end):
        if chars[index] != "\n":
            chars[index] = " "


def mask_non_code(source: str) -> str:
    """Mask Rust comments and literals without changing byte positions."""

    chars = list(source)
    index = 0
    size = len(source)
    while index < size:
        if source.startswith("//", index):
            end = source.find("\n", index + 2)
            if end < 0:
                end = size
            _blank(chars, index, end)
            index = end
            continue

        if source.startswith("/*", index):
            depth = 1
            end = index + 2
            while end < size and depth:
                if source.startswith("/*", end):
                    depth += 1
                    end += 2
                elif source.startswith("*/", end):
                    depth -= 1
                    end += 2
                else:
                    end += 1
            if depth:
                raise ValueError("unterminated block comment")
            _blank(chars, index, end)
            index = end
            continue

        raw = RAW_STRING_START.match(source, index)
        if raw:
            delimiter = '"' + raw.group(1)
            end_marker = source.find(delimiter, raw.end())
            if end_marker < 0:
                raise ValueError("unterminated raw string")
            end = end_marker + len(delimiter)
            _blank(chars, index, end)
            index = end
            continue

        if source[index] == '"':
            end = index + 1
            escaped = False
            while end < size:
                char = source[end]
                if char == '"' and not escaped:
                    end += 1
                    break
                if char == "\n" and not escaped:
                    raise ValueError("unterminated string literal")
                if char == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                end += 1
            else:
                raise ValueError("unterminated string literal")
            _blank(chars, index, end)
            index = end
            continue

        char_literal = CHAR_LITERAL.match(source, index)
        if char_literal:
            _blank(chars, index, char_literal.end())
            index = char_literal.end()
            continue

        index += 1

    return "".join(chars)


def brace_pairs(code: str) -> dict[int, int]:
    stack: list[int] = []
    pairs: dict[int, int] = {}
    for index, char in enumerate(code):
        if char == "{":
            stack.append(index)
        elif char == "}":
            if not stack:
                raise ValueError(f"unmatched closing brace at byte {index}")
            pairs[stack.pop()] = index
    if stack:
        raise ValueError(f"unmatched opening brace at byte {stack[-1]}")
    return pairs


def function_spans(code: str, pairs: dict[int, int]) -> list[tuple[int, int, str]]:
    """Return named function body spans found in masked Rust source."""

    openings = sorted(pairs)
    spans: list[tuple[int, int, str]] = []
    for match in FUNCTION_NAME.finditer(code):
        opening = next((item for item in openings if item > match.end()), None)
        if opening is None:
            continue
        semicolon = code.find(";", match.end(), opening)
        if semicolon >= 0:
            continue
        spans.append((opening, pairs[opening], match.group(1)))
    return spans


def containing_span(
    position: int, spans: Iterable[tuple[int, int, str]]
) -> tuple[int, int, str] | None:
    matches = [span for span in spans if span[0] < position < span[1]]
    return max(matches, key=lambda span: span[0], default=None)


def receiver_pattern(receiver: str, field: str) -> re.Pattern[str]:
    pieces = [re.escape(piece) for piece in receiver.split(".")]
    receiver_expr = r"\s*\.\s*".join(pieces)
    return re.compile(
        rf"(?<![A-Za-z0-9_]){receiver_expr}\s*\.\s*{field}"
        rf"\s*=\s*true\b"
    )


def scan_source(path: str, source: str) -> list[Site]:
    code = mask_non_code(source)
    pairs = brace_pairs(code)
    spans = function_spans(code, pairs)
    openings = sorted(pairs)
    sites: list[Site] = []

    for assignment in OPEN_ASSIGNMENT.finditer(code):
        line = source.count("\n", 0, assignment.start()) + 1
        function = containing_span(assignment.start(), spans)
        function_name = function[2] if function else "<outside-function>"

        receiver_match = SIMPLE_RECEIVER.search(code[: assignment.start()])
        receiver = None
        if receiver_match:
            receiver = re.sub(r"\s+", "", receiver_match.group(1))

        blocks = [
            (opening, pairs[opening])
            for opening in openings
            if opening < assignment.start() < pairs[opening]
        ]
        block = max(blocks, key=lambda item: item[0], default=None)
        paired = False
        if receiver is not None and block is not None:
            paired = bool(
                receiver_pattern(receiver, "has_opened").search(
                    code, block[0] + 1, block[1]
                )
            )

        sites.append(
            Site(
                path=path,
                function=function_name,
                receiver=receiver,
                line=line,
                paired=paired,
            )
        )
    return sites


def parse_exceptions(data: Any) -> tuple[list[ExceptionEntry], list[str]]:
    errors: list[str] = []
    if not isinstance(data, dict) or data.get("schema_version") != 1:
        return [], ["exception registry must be an object with schema_version 1"]
    raw_entries = data.get("exceptions")
    if not isinstance(raw_entries, list):
        return [], ["exception registry field 'exceptions' must be a list"]

    entries: list[ExceptionEntry] = []
    required = ("name", "path", "function", "receiver", "reason")
    for index, raw in enumerate(raw_entries):
        if not isinstance(raw, dict):
            errors.append(f"exception {index} must be an object")
            continue
        missing = [
            key
            for key in required
            if not isinstance(raw.get(key), str) or not raw[key].strip()
        ]
        if missing:
            errors.append(f"exception {index} has missing/empty fields: {', '.join(missing)}")
            continue
        if len(raw["reason"].strip()) < 20:
            errors.append(
                f"exception {raw['name']!r} needs a specific reason of at least 20 characters"
            )
            continue
        entries.append(
            ExceptionEntry(**{key: raw[key].strip() for key in required})
        )

    names: set[str] = set()
    keys: set[tuple[str, str, str]] = set()
    for entry in entries:
        if entry.name in names:
            errors.append(f"duplicate exception name: {entry.name}")
        names.add(entry.name)
        if entry.key in keys:
            errors.append(
                "duplicate exception identity: " + "::".join(entry.key)
            )
        keys.add(entry.key)
    return entries, errors


def evaluate(
    sources: dict[str, str],
    registry: Any,
    *,
    min_assignments: int = MIN_ASSIGNMENTS,
) -> tuple[list[Site], list[str]]:
    entries, errors = parse_exceptions(registry)
    sites: list[Site] = []
    for path, source in sorted(sources.items()):
        try:
            sites.extend(scan_source(path, source))
        except ValueError as error:
            errors.append(f"{path}: source scan failed: {error}")

    if len(sites) < min_assignments:
        errors.append(
            f"matched only {len(sites)} is_open=true assignment(s), below the "
            f"coverage floor of {min_assignments}; the scanner or source population changed"
        )

    unpaired_by_key: dict[tuple[str, str, str], list[Site]] = {}
    for site in sites:
        if site.receiver is None:
            errors.append(
                f"{site.path}:{site.line}: is_open=true uses receiver syntax the "
                "checker cannot prove; keep the receiver as a simple identifier/field path"
            )
        elif not site.paired:
            assert site.key is not None
            unpaired_by_key.setdefault(site.key, []).append(site)

    entries_by_key = {entry.key: entry for entry in entries}
    for key, matches in sorted(unpaired_by_key.items()):
        if len(matches) > 1:
            locations = ", ".join(str(site.line) for site in matches)
            errors.append(
                f"{'::'.join(key)}: exception identity is ambiguous across lines "
                f"{locations}; each intentional transition needs a unique function/receiver"
            )
            continue
        if key not in entries_by_key:
            site = matches[0]
            errors.append(
                f"{site.path}:{site.line}: {site.receiver}.is_open=true in "
                f"{site.function} does not set {site.receiver}.has_opened=true in "
                "the same block and has no documented exception"
            )

    for entry in entries:
        matches = unpaired_by_key.get(entry.key, [])
        if len(matches) != 1:
            errors.append(
                f"exception {entry.name!r} ({'::'.join(entry.key)}) matches "
                f"{len(matches)} unpaired transition(s); delete stale exceptions and "
                "give each live exception a unique identity"
            )

    return sites, errors


def load_sources() -> dict[str, str]:
    return {
        path.relative_to(ROOT).as_posix(): path.read_text(encoding="utf-8")
        for path in sorted(SOURCE_ROOT.rglob("*.rs"))
    }


def load_registry() -> Any:
    return json.loads(EXCEPTIONS_PATH.read_text(encoding="utf-8"))


def self_test() -> None:
    path = "crates/perry-ext-net/src/example.rs"
    exception = {
        "name": "synthetic-open",
        "path": path,
        "function": "open",
        "receiver": "socket",
        "reason": "Synthetic socket stays live only to deliver a deferred close event.",
    }

    paired = """
fn open() {
    if ready {
        socket.is_open = true;
        socket.has_opened = true;
    }
}
"""
    sites, errors = evaluate(
        {path: paired}, {"schema_version": 1, "exceptions": []}, min_assignments=1
    )
    assert not errors and len(sites) == 1 and sites[0].paired

    missing = paired.replace("        socket.has_opened = true;\n", "")
    _, errors = evaluate(
        {path: missing}, {"schema_version": 1, "exceptions": []}, min_assignments=1
    )
    assert any("no documented exception" in error for error in errors)

    _, errors = evaluate(
        {path: missing},
        {"schema_version": 1, "exceptions": [exception]},
        min_assignments=1,
    )
    assert not errors

    _, errors = evaluate(
        {path: paired},
        {"schema_version": 1, "exceptions": [exception]},
        min_assignments=1,
    )
    assert any("matches 0 unpaired" in error for error in errors)

    sibling = """
fn open() {
    if ready { socket.is_open = true; }
    if history { socket.has_opened = true; }
}
"""
    _, errors = evaluate(
        {path: sibling}, {"schema_version": 1, "exceptions": []}, min_assignments=1
    )
    assert any("same block" in error for error in errors)

    masked = r'''
fn harmless() {
    // socket.is_open = true;
    let a = "socket.is_open = true; {";
    let b = r#"socket.is_open = true; }"#;
    let c = '{';
    /* nested /* socket.is_open = true; */ comment */
}
'''
    sites, errors = evaluate(
        {path: masked}, {"schema_version": 1, "exceptions": []}, min_assignments=0
    )
    assert not sites and not errors

    complex_receiver = "fn open() { (*socket).is_open = true; }"
    _, errors = evaluate(
        {path: complex_receiver},
        {"schema_version": 1, "exceptions": []},
        min_assignments=1,
    )
    assert any("receiver syntax" in error for error in errors)

    ambiguous = """
fn open() {
    if first { socket.is_open = true; }
    if second { socket.is_open = true; }
}
"""
    _, errors = evaluate(
        {path: ambiguous},
        {"schema_version": 1, "exceptions": [exception]},
        min_assignments=2,
    )
    assert any("ambiguous" in error for error in errors)

    bad_reason = dict(exception, reason="too short")
    _, errors = evaluate(
        {path: missing},
        {"schema_version": 1, "exceptions": [bad_reason]},
        min_assignments=1,
    )
    assert any("specific reason" in error for error in errors)

    print(
        "check_ext_net_socket_open_invariant self-test: OK "
        "(pair, missing, exception, stale, scope, masking, syntax, ambiguity)"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    try:
        sites, errors = evaluate(load_sources(), load_registry())
    except (OSError, json.JSONDecodeError) as error:
        print(f"check_ext_net_socket_open_invariant: {error}", file=sys.stderr)
        return 2

    if args.list:
        for site in sites:
            receiver = site.receiver or "<complex>"
            status = "paired" if site.paired else "exception candidate"
            print(f"{site.path}:{site.line} {site.function} {receiver} {status}")

    if errors:
        print("check_ext_net_socket_open_invariant FAILED:", file=sys.stderr)
        for error in errors:
            print(f"  - {error}", file=sys.stderr)
        return 1

    paired = sum(site.paired for site in sites)
    print(
        "check_ext_net_socket_open_invariant: OK — "
        f"{len(sites)} is_open=true assignment(s), {paired} paired, "
        f"{len(sites) - paired} documented exception(s)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
