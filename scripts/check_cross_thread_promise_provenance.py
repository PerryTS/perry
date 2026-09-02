#!/usr/bin/env python3
"""#9552 — arena promises must not be handed to native settlement paths.

A promise whose address leaves the runtime as a bare integer (a tokio future,
`std::thread::spawn`, a pending-result queue) is invisible to every root
scanner until its completion is queued back. Two constructors exist:

  * `js_promise_new_cross_thread()` — malloc-resident (non-moving) AND pinned
    by the constructor until the promise settles (#9552). Safe to hand off.
  * `js_promise_new()` / `js_promise_new_with_parent(..)` — nursery-resident.
    A copying minor relocates it behind the worker's back, and a full
    collection frees it. NEVER safe to hand off.

This gate finds functions that mint a promise with an ARENA constructor and
pass it (directly, via `as usize`, or via a `let p = promise as usize` alias)
into a native settlement sink, or capture it in a spawned closure/future.
Sinks are matched by name, so a new hand-off API must be added to `SINKS`.

Exit 1 on any hit. `--self-test` proves the detector can still fail: it plants
the bad shape (and its alias/spawn variants) and asserts each is reported, and
plants the good shape and asserts it is not.
"""
from __future__ import annotations

import argparse
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
SCAN_DIRS = ["crates/perry-runtime/src", "crates/perry-stdlib/src"] + [
    str(p.relative_to(ROOT)) for p in sorted(ROOT.glob("crates/perry-ext-*/src"))
]

PROMISE_BINDING = re.compile(
    r"\blet\s+(?:mut\s+)?(?P<name>\w+)\s*(?::\s*[^=]+)?=\s*(?:[\w:]+::)?"
    r"(?P<ctor>js_promise_new(?:_with_parent|_cross_thread|_for_native_resolution)?)\s*\("
)
ARENA_CTORS = {"js_promise_new", "js_promise_new_with_parent"}
ALIAS = re.compile(r"\blet\s+(?:mut\s+)?(?P<alias>\w+)\s*(?::\s*usize)?\s*=\s*(?P<src>\w+)\s+as\s+usize\s*;")
SINKS = [
    "queue_promise_resolution",
    "queue_deferred_resolution",
    "queue_promise_string_result",
    "queue_thread_result",
    "spawn_for_promise",
    "spawn_for_promise_deferred",
    "perry_ffi_spawn_blocking",
    "perry_ffi_spawn_async",
    "spawn_blocking",
    "spawn",
]
SINK_CALL = re.compile(r"\b(?:[\w:]+::)?(?P<sink>" + "|".join(map(re.escape, SINKS)) + r")\s*\(")
FN_HEADER = re.compile(r"\bfn\s+(?P<fn>\w+)\s*(?:<[^>]*>)?\s*\(")


def strip_comments(src: str) -> str:
    """Blank out `//` comments (keeping line structure) so doc prose cannot
    trip the matchers."""
    out = []
    for line in src.split("\n"):
        cut = line.find("//")
        if cut != -1 and line.count('"', 0, cut) % 2 == 0:
            line = line[:cut]
        out.append(line)
    return "\n".join(out)


def match_brace(src: str, open_idx: int) -> int:
    """Index just past the `}` matching the `{` at `open_idx` (or the `)` for
    `(`), ignoring string literals."""
    opener = src[open_idx]
    closer = {"{": "}", "(": ")"}[opener]
    depth = 0
    i = open_idx
    in_str = False
    while i < len(src):
        c = src[i]
        if in_str:
            if c == "\\":
                i += 2
                continue
            if c == '"':
                in_str = False
        elif c == '"':
            in_str = True
        elif c == opener:
            depth += 1
        elif c == closer:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return len(src)


def function_bodies(src: str):
    """Yield (fn_name, body_start, body_text) for every fn in `src`."""
    for m in FN_HEADER.finditer(src):
        sig_end = match_brace(src, m.end() - 1)
        brace = src.find("{", sig_end)
        semi = src.find(";", sig_end)
        if brace == -1 or (semi != -1 and semi < brace):
            continue  # trait method without a body
        end = match_brace(src, brace)
        yield m.group("fn"), brace, src[brace:end]


def latest_before(entries, name, pos):
    """The last (pos, value) entry for `name` that precedes `pos`, or None.
    Bindings shadow: an early-return arena promise named `promise` must not
    taint a later `let promise = js_promise_new_cross_thread()`."""
    best = None
    for entry_pos, value in entries.get(name, ()):
        if entry_pos < pos and (best is None or entry_pos > best[0]):
            best = (entry_pos, value)
    return best


def analyze_source(src: str, path: str = "<memory>"):
    """Return a list of (line, fn, message) findings for one Rust source."""
    src = strip_comments(src)
    findings = []
    for fn_name, body_start, body in function_bodies(src):
        bindings = {}
        for m in PROMISE_BINDING.finditer(body):
            bindings.setdefault(m.group("name"), []).append((m.start(), m.group("ctor")))
        if not any(ctor in ARENA_CTORS for entries in bindings.values() for _, ctor in entries):
            continue
        aliases = {}
        for m in ALIAS.finditer(body):
            aliases.setdefault(m.group("alias"), []).append((m.start(), m.group("src")))
        names = set(bindings) | set(aliases)
        ident = re.compile(r"\b(" + "|".join(map(re.escape, sorted(names))) + r")\b")
        for call in SINK_CALL.finditer(body):
            args_end = match_brace(body, call.end() - 1)
            args = body[call.end() - 1 : args_end]
            for hit in ident.finditer(args):
                name, pos = hit.group(1), call.start()
                alias = latest_before(aliases, name, pos)
                if alias is not None:
                    pos, name = alias
                binding = latest_before(bindings, name, pos)
                if binding is None or binding[1] not in ARENA_CTORS:
                    continue
                line = src.count("\n", 0, body_start + call.start()) + 1
                findings.append(
                    (
                        line,
                        fn_name,
                        f"arena promise `{hit.group(1)}` (from {binding[1]}) reaches native sink "
                        f"`{call.group('sink')}`; mint it with js_promise_new_cross_thread() instead",
                    )
                )
                break
    return findings


def scan_tree():
    findings = []
    for rel in SCAN_DIRS:
        for path in sorted((ROOT / rel).rglob("*.rs")):
            for line, fn_name, msg in analyze_source(path.read_text(), str(path)):
                findings.append(f"{path.relative_to(ROOT)}:{line}: in `{fn_name}`: {msg}")
    return findings


BAD_DIRECT = """
pub unsafe extern "C" fn bad_direct() -> *mut Promise {
    let promise = perry_runtime::js_promise_new();
    queue_promise_resolution(promise as usize, true, 0);
    promise
}
"""
BAD_ALIAS_SPAWN = """
pub unsafe extern "C" fn bad_alias() -> *mut Promise {
    let promise = js_promise_new();
    let promise_ptr = promise as usize;
    spawn(async move {
        queue_deferred_resolution(promise_ptr, true, || 0);
    });
    promise
}
"""
BAD_WITH_PARENT = """
fn bad_parent(parent: *mut Promise) {
    let p = crate::promise::js_promise_new_with_parent(parent);
    let raw = p as usize;
    std::thread::spawn(move || queue_promise_string_result(0, raw, String::new()));
}
"""
GOOD_CROSS_THREAD = """
pub unsafe extern "C" fn good() -> *mut Promise {
    let promise = perry_runtime::js_promise_new_cross_thread();
    let promise_ptr = promise as usize;
    spawn(async move { queue_promise_resolution(promise_ptr, true, 0); });
    promise
}
"""
GOOD_SHADOWED = """
unsafe fn good_shadowed(closure: *const u8) -> *mut Promise {
    if closure.is_null() {
        let promise = crate::promise::js_promise_new();
        crate::promise::js_promise_resolve(promise, 0.0);
        return promise;
    }
    let promise = crate::promise::js_promise_new_cross_thread();
    let promise_usize = promise as usize;
    std::thread::spawn(move || queue_thread_result(0, promise_usize, 0));
    promise
}
"""
GOOD_UNRELATED = """
fn good_unrelated(writable_id: usize) -> *mut Promise {
    let promise = js_promise_new();
    // a different usize reaches the sink; the promise stays on the main thread
    let job = writable_id as usize;
    spawn(async move { finish(job) });
    promise
}
"""


def self_test() -> int:
    failures = []
    for label, snippet, expect in [
        ("direct", BAD_DIRECT, True),
        ("alias+spawn", BAD_ALIAS_SPAWN, True),
        ("with_parent+thread::spawn", BAD_WITH_PARENT, True),
        ("cross_thread ctor", GOOD_CROSS_THREAD, False),
        ("unrelated usize", GOOD_UNRELATED, False),
        ("shadowed early-return arena promise", GOOD_SHADOWED, False),
    ]:
        got = bool(analyze_source(snippet))
        if got != expect:
            failures.append(f"{label}: expected {'a hit' if expect else 'no hit'}, got {analyze_source(snippet)}")
    if failures:
        print("check_cross_thread_promise_provenance --self-test FAILED:")
        for f in failures:
            print("  " + f)
        return 1
    print("check_cross_thread_promise_provenance --self-test ok (3 planted shapes caught, 3 clean shapes pass)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    findings = scan_tree()
    if findings:
        print("#9552 cross-thread promise provenance: arena promises handed to native settlement paths:")
        for f in findings:
            print("  " + f)
        print(f"{len(findings)} finding(s). Mint with js_promise_new_cross_thread() (pinned until settled).")
        return 1
    print("check_cross_thread_promise_provenance: no arena promise reaches a native settlement sink")
    return 0


if __name__ == "__main__":
    sys.exit(main())
