#!/usr/bin/env python3
"""Audit net.Socket's single payload opened field (#12064).

Reject second open flags, JS-state open latches, and id-keyed map/set tables
anywhere in perry-ext-net. Comments and literals are masked for Rust syntax;
JS own-property writes are inspected separately. The payload field and its
three production transitions must exist, so an empty census cannot pass.
"""
from __future__ import annotations
import argparse
import json
import re
import sys
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent
EXCEPTIONS_PATH = ROOT / "scripts/ext_net_socket_open_exceptions.json"
CHAR_LITERAL = re.compile(r"'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}|.)|[^'\\\n])'")
RAW_STRING_START = re.compile(r'(?:b|c)?r(#{0,255})"')


def _blank(chars: list[str], start: int, end: int) -> None:
    """Replace a non-code range with spaces while preserving newlines."""

    for index in range(start, end):
        if chars[index] != "\n":
            chars[index] = " "


def mask_non_code(source: str, keep_strings: bool = False) -> str:
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
            if not keep_strings:
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
                if char == "\\" and not escaped:
                    escaped = True
                else:
                    escaped = False
                end += 1
            else:
                raise ValueError("unterminated string literal")
            if not keep_strings:
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



OPEN_NAME = re.compile(r"open", re.I)
FIELD = re.compile(r"\b(\w+)\s*:\s*(?:bool|Atomic\w+|Cell|Option|u8|u32|u64|usize|i32|i64)\b")
TABLE = re.compile(r"\b(?:HashMap|BTreeMap|DashMap|IndexMap|HashSet|BTreeSet|Slab)\b")
CANONICAL = ("crates/perry-ext-net/src/payload_transport.rs", "opened")

def evaluate(sources, registry):
    errors, sites, used = [], [], set()
    exceptions = {(e["path"], e["name"]): e for e in registry["exceptions"]}
    canonical = 0
    transitions = 0
    for path, source in sources.items():
        code = mask_non_code(source)
        for match in FIELD.finditer(code):
            name = match[1]
            if not OPEN_NAME.search(name):
                continue
            key = path, name
            sites.append(f"{path}:{code[:match.start()].count(chr(10))+1}: {name}")
            if key == CANONICAL:
                canonical += 1
            elif key in exceptions and len(exceptions[key].get("reason", "")) >= 40:
                used.add(key)
            else:
                errors.append(f"{path}: second open flag {name}")
        if TABLE.search(code):
            errors.append(f"{path}: id-keyed map/set table in payload-only net crate")
        for match in re.finditer(r"\.\s*(\w*open\w*)\s*=(?!=)", code, re.I):
            if match[1] not in {"opened", "allow_half_open"}:
                errors.append(f"{path}: second open assignment {match[1]}")
            if match[1] == "opened" and re.match(r"\s*true\b", code[match.end():]):
                transitions += 1
        # A JS-visible hidden latch is still duplicate state. Scan property
        # writes using original literal keys, after removing comments.
        for match in re.finditer(r'\bown_set\s*\([^;]*?"([^"\n]*open[^"\n]*)"', mask_non_code(source, keep_strings=True), re.I):
            if match[1] != "allowHalfOpen":
                errors.append(f"{path}: duplicate JS open property {match[1]}")
    if canonical != 1:
        errors.append(f"expected exactly one SocketFields.opened declaration, found {canonical}")
    if transitions < 3:
        errors.append(f"opened transition census {transitions} below three production transitions")
    for key in exceptions.keys() - used:
        errors.append(f"stale exception: {key}")
    return sites, errors

def self_test():
    good = {CANONICAL[0]: "struct SocketFields { opened: bool } fn connected() { s.opened = true; s.opened = true; s.opened = true; }"}
    registry = {"exceptions": []}
    assert not evaluate(good, registry)[1]
    for bad in ["struct Second { is_open: bool }", "struct Second { has_opened: bool }", "static OPEN: AtomicBool = AtomicBool::new(false);", "struct Second { opened: Option<bool> }", "struct Second { is_open: u8 }", "static FLAGS: HashMap<u64, bool> = todo!();", 'fn f() { p::own_set(owner, "bunOpened", true); }']:
        assert evaluate(dict(good, extra=bad), registry)[1], bad
    assert evaluate({}, registry)[1]
    assert evaluate(good, {"exceptions": [{"path": "gone", "name": "open", "reason": "x"*50}]})[1]
    assert not evaluate(dict(good, extra='// own_set(owner, "bunOpened", true);\n// is_open: bool\n/* HashMap<u64, bool> */ fn f() { let s = "has_opened: bool"; }'), registry)[1]
    print("socket-open self-test: OK (duplicate fields, atomic flag, table, JS latch, empty census, stale exception, masking)")

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--list", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    sources = {str(p.relative_to(ROOT)): p.read_text() for p in (ROOT / "crates/perry-ext-net/src").rglob("*.rs")}
    sites, errors = evaluate(sources, json.loads(EXCEPTIONS_PATH.read_text()))
    if args.list:
        print("\n".join(sites))
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("socket-open invariant: OK; one payload opened field, no second flag or id-keyed open table")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
