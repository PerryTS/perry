#!/usr/bin/env python3
"""Keep host-locale text I/O out of the scripts CI runs on Windows (#7977).

WHY THIS EXISTS
===============

`open()`, `Path.read_text()` and `Path.write_text()` decode and encode with
`locale.getencoding()` when no `encoding=` is given. On Linux and macOS that is
UTF-8, so a bare call is indistinguishable from a correct one; on a GitHub
Windows runner it is **cp1252**, which has no mapping for 0x81/0x8d/0x8f/0x90/
0x9d. Fifteen files under `crates/perry-runtime/src` contain one of those bytes.

`scripts/check_thread_locals.py::cfg_test_module_files` walked the crate with a
bare `read_text()` and died on `i18n.rs` with `UnicodeDecodeError: 'charmap'
codec can't decode byte 0x8d in position 32475` — offset 31552 plus the 923
newlines `core.autocrlf` had widened, to the byte. That was the FIRST step of
`windows-build`, so the eleven steps behind it — including the only Windows run
of the `perry-runtime` unit tests — were `skipped` on every PR.

#7882 ("make GC structural audits portable") fixed the path-separator half of
this class and the encoding in `gc_runtime_root_holders.py`, but missed this one
call site. This checker is what stops the next miss: a locale-dependent call is
now a **Linux** failure in the per-PR `lint` job, so it can no longer reach
Windows and take the job down before the audits it gates have run.

WHY STATIC AND NOT `PYTHONWARNDEFAULTENCODING`
==============================================

Python's own `EncodingWarning` (`-X warn_default_encoding`) reports the same
defect, and is used here as the self-test's oracle. But it only fires on a call
that actually *executes*: a bare `read_text()` on an error path, or in a
subcommand CI does not invoke, warns nobody and ships. The scan below reads the
AST, so an unexecuted branch is caught exactly like a hot one.

Strings are not code: an `open(...)` inside a probe's source-code *literal* (as
in `tests/test_gc_ratchet.py`, which writes Python probes as text) is invisible
to the AST and correctly not flagged. That is the second reason not to grep.

THIS CHECK IS DESIGNED TO BE ABLE TO FAIL
=========================================

`--self-test` plants each flagged shape and each accepted shape in a synthetic
module and asserts the scan separates them, so the checker cannot quietly stop
being able to say no. It also asserts the scanned file list is non-empty and
that every named file exists — a scope that silently shrinks to nothing is
CLAUDE.md's fourth way a gate cannot fail.
"""

from __future__ import annotations

import argparse
import ast
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Exactly the Python that `windows-build`'s "GC structural audits (Windows)"
# step executes, plus this checker. Keep in lockstep with that step: a script
# added there and not here is unguarded, and `--self-test` fails if a name here
# stops existing.
SCANNED = (
    "scripts/check_locale_independent_io.py",
    "scripts/check_thread_locals.py",
    "scripts/check_gc_doc_claims.py",
    "scripts/gc_runtime_root_holders.py",
    "benchmarks/gc_ratchet/gc_ratchet.py",
    "tests/test_gc_ratchet.py",
)

# `open(...)` and the `Path` text helpers. `Path.open()` is included: it is the
# same defaulting. Binary modes are exempt (there is no encoding to get wrong),
# which the mode check below establishes.
TEXT_METHODS = {"read_text", "write_text", "open"}


def _keyword(call: ast.Call, name: str) -> ast.expr | None:
    for kw in call.keywords:
        if kw.arg == name:
            return kw.value
        if kw.arg is None:  # `**kwargs` — cannot prove absence, treat as given.
            return kw.value
    return None


def _is_binary(call: ast.Call, func_name: str) -> bool:
    """True when the call cannot have an encoding (binary mode)."""
    if func_name == "read_text" or func_name == "write_text":
        return False
    mode: ast.expr | None = _keyword(call, "mode")
    if mode is None:
        # Positional mode: `open(p, "rb")` / `p.open("rb")`.
        idx = 1 if func_name == "open" and not _is_method(call) else 0
        if func_name == "open" and _is_method(call):
            idx = 0
        elif func_name == "open":
            idx = 1
        if len(call.args) > idx:
            mode = call.args[idx]
    return isinstance(mode, ast.Constant) and isinstance(mode.value, str) and "b" in mode.value


def _is_method(call: ast.Call) -> bool:
    return isinstance(call.func, ast.Attribute)


def _func_name(call: ast.Call) -> str | None:
    if isinstance(call.func, ast.Attribute):
        return call.func.attr
    if isinstance(call.func, ast.Name):
        return call.func.id
    return None


def scan_source(src: str, rel: str) -> list[str]:
    """Flagged call sites in one module, as human-readable problems."""
    problems: list[str] = []
    tree = ast.parse(src)
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        name = _func_name(node)
        if name not in TEXT_METHODS:
            continue
        # A bare `open(...)` that is not `builtins.open` (e.g. `zipfile.open`)
        # still defaults the same way when it is a text stream; flagging it is
        # the safe direction and the fix is identical.
        if _is_binary(node, name):
            continue
        if _keyword(node, "encoding") is not None:
            continue
        problems.append(
            f"{rel}:{node.lineno}: `{name}(...)` without `encoding=`. "
            f"It decodes with the host locale — cp1252 on a Windows runner, "
            f"which cannot read the 15 runtime sources carrying 0x81/0x8d/"
            f"0x8f/0x90/0x9d (#7977). Pass `encoding=\"utf-8\"`."
        )
    return sorted(problems)


def verify(root: Path, scanned: tuple[str, ...]) -> list[str]:
    problems: list[str] = []
    if not scanned:
        return ["nothing was scanned — the gate's scope is empty (see hazard 4)"]
    for rel in scanned:
        path = root / rel
        if not path.exists():
            problems.append(
                f"{rel}: listed in SCANNED but missing. Either it moved (update "
                f"the list) or the Windows audit step no longer runs it — a "
                f"stale entry is scope nobody has to justify."
            )
            continue
        problems.extend(scan_source(path.read_text(encoding="utf-8"), rel))
    return problems


GOOD_SOURCE = '''
import io
from pathlib import Path

def ok(p: Path, q):
    a = p.read_text(encoding="utf-8")
    p.write_text(a, encoding="utf-8", newline="")
    with open(q, "r", encoding="utf-8") as fh:
        fh.read()
    with open(q, "rb") as fh:          # binary: no encoding to get wrong
        fh.read()
    with p.open("rb") as fh:
        fh.read()
    p.write_bytes(b"x")
    embedded = """
        body = open(source).read()
        with open(out, "w") as handle:
            handle.write(body)
    """                                # a STRING, not a call — must not flag
    return a, embedded
'''

BAD_SHAPES = (
    ("read_text", "def f(p):\n    return p.read_text()\n"),
    ("write_text", "def f(p):\n    p.write_text('x')\n"),
    ("open-builtin", "def f(q):\n    return open(q).read()\n"),
    ("open-mode-w", "def f(q):\n    return open(q, 'w')\n"),
    ("path-open", "def f(p):\n    return p.open()\n"),
    ("unreached", "def f(p):\n    if False:\n        return p.read_text()\n    return ''\n"),
)


def self_test() -> int:
    failures: list[str] = []

    if scan_source(GOOD_SOURCE, "good.py"):
        failures.append(
            "the accepted shapes were flagged: " + "; ".join(scan_source(GOOD_SOURCE, "good.py"))
        )
    for label, src in BAD_SHAPES:
        if not scan_source(src, "bad.py"):
            failures.append(f"a bare `{label}` call passed the scan")

    # Scope liveness: an empty or stale list is the failure mode this gate is
    # least able to notice about itself.
    if verify(REPO, ()) == []:
        failures.append("an empty scan scope was reported clean")
    missing = verify(REPO, ("scripts/definitely-not-here.py",))
    if not missing:
        failures.append("a missing scanned file was reported clean")

    if failures:
        for f in failures:
            print(f"self-test FAILED: {f}", file=sys.stderr)
        return 1
    print(
        f"check_locale_independent_io self-test: OK "
        f"({len(BAD_SHAPES)} flagged shapes, accepted shapes clean, scope asserted)"
    )
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--self-test", action="store_true", help="prove the checker can say no")
    args = parser.parse_args()
    if args.self_test:
        return self_test()

    problems = verify(REPO, SCANNED)
    if problems:
        print("locale-dependent text I/O in a Windows-CI script:\n", file=sys.stderr)
        for p in problems:
            print(f"  {p}", file=sys.stderr)
        print(
            "\nThese run in `windows-build`'s GC structural audits, which is the "
            "FIRST step of the job — a UnicodeDecodeError there skips the only "
            "Windows run of the perry-runtime unit tests (#7977).",
            file=sys.stderr,
        )
        return 1
    print(f"locale-independent I/O OK: {len(SCANNED)} Windows-CI scripts scanned")
    return 0


if __name__ == "__main__":
    sys.exit(main())
