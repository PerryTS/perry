#!/usr/bin/env python3
"""Every call of a JS function body goes through ONE funnel per side.

A JS body is the native code a function object runs (a compiled closure body,
a value wrapper, a native builtin), and the class-method / static-method
bodies the runtime calls by raw pointer. Their calling convention is about to
change (the receiver becomes a parameter), so the calls are funneled:

* runtime: only `crates/perry-runtime/src/closure/body_call.rs` may turn a
  code pointer into a callable body type (`mem::transmute` to an
  `fn(...) -> f64` whose parameters are all `f64` / `*const ClosureHeader`);
* codegen: only `crates/perry-codegen/src/expr/body_call.rs` (and the block
  builder that defines them) may emit an indirect call.

A transmute FROM a body type to an erased pointer (installing a native body)
is not a call and is not this gate's business.

The receiver is a parameter of every JS body (`perry_abi::JS_BODY_THIS_PARAM`,
this-as-a-parameter stage 1), and a native body is installed through an
erased `*const u8`, so nothing the compiler checks connects a native's
signature to the ABI it is called with. Two definition rules close that:

* every `extern "C" fn` DEFINITION shaped like a JS body — its first
  parameter a closure header, every other parameter `f64`, returning `f64` —
  declares the receiver (`JsThis`) as its second parameter, unless it is a
  call ENTRY (`perry_abi::JS_CALL_ENTRIES`: it takes a closure as an ordinary
  argument) or says `NOT-A-JS-BODY: <why>` on one of the three lines above;
* a function whose address is handed straight to a closure allocator
  (`js_closure_alloc*`, `js_register_closure_*`, perry-ffi's `alloc_closure` /
  `register_closure_arity`) declares the receiver.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

RUNTIME_ROOTS = ("crates/perry-runtime/src", "crates/perry-stdlib/src")  # + crates/perry-ext-*/src
RUNTIME_FUNNEL = "crates/perry-runtime/src/closure/body_call.rs"
CODEGEN_ROOT = "crates/perry-codegen/src"
CODEGEN_ALLOWED = {
    "crates/perry-codegen/src/expr/body_call.rs",  # the funnel
    "crates/perry-codegen/src/block.rs",  # defines call_indirect (+ its unit tests)
    "crates/perry-codegen/src/gc_call_effects.rs",  # effect-classifier unit tests only
}

TRANSMUTE_RE = re.compile(r"\btransmute\b")
# A callable JS/method/static body type: returns f64, and every parameter is
# f64 or a closure-header pointer.
PARAM = r"\s*(?:f64|\*const\s+(?:crate::closure::|super::|perry_runtime::closure::)?ClosureHeader)\s*"
BODY_TYPE_RE = re.compile(
    r'(?:unsafe\s+)?extern\s+"C(?:-unwind)?"\s+fn\s*\((?:' + PARAM + r"(?:," + PARAM + r")*,?)?\s*\)\s*->\s*f64"
)
TURBOFISH_RE = re.compile(r"transmute\s*::\s*<(.*)>\s*\(", re.S)
LET_TYPE_RE = re.compile(r"let\s+(?:mut\s+)?\w+\s*:\s*([^=]+)=\s*(?:unsafe\s*\{\s*)?(?:std::mem::|::std::mem::|mem::)?transmute\b", re.S)
ALIAS_RE = re.compile(r"type\s+(\w+)\s*=\s*([^;]+);")


def split_turbofish(args: str) -> str:
    """Return the target (second) type of `transmute::<A, B>`."""
    depth = 0
    for i, ch in enumerate(args):
        if ch in "(<[":
            depth += 1
        elif ch in ")>]":
            depth -= 1
        elif ch == "," and depth == 0:
            return args[i + 1 :]
    return ""


def transmute_targets(text: str):
    """Yield (line, target_type_text) for every transmute call in `text`."""
    aliases = {m.group(1): m.group(2) for m in ALIAS_RE.finditer(text)}
    for m in TRANSMUTE_RE.finditer(text):
        start = m.start()
        line = text.count("\n", 0, start) + 1
        tail = text[start : start + 600]
        target = ""
        tf = TURBOFISH_RE.match(tail)
        if tf:
            inner = tf.group(1)
            # cut at the matching `>` of the turbofish
            depth = 0
            for i, ch in enumerate(inner):
                if ch == "<":
                    depth += 1
                elif ch == ">" and not (i > 0 and inner[i - 1] == "-"):
                    if depth == 0:
                        inner = inner[:i]
                        break
                    depth -= 1
            target = split_turbofish(inner)
        else:
            window = text[max(0, start - 400) : start + len("transmute")]
            stmt = window[window.rfind(";") + 1 :]
            lt = LET_TYPE_RE.search(stmt)
            if lt:
                target = lt.group(1)
        target = aliases.get(target.strip(), target)
        yield line, target


NOT_A_BODY = "NOT-A-JS-BODY:"

# Rust definitions (never a pointer type: those have no name).
FN_HEAD_RE = re.compile(r'extern\s+"C(?:-unwind)?"\s+fn\s+(\$?\w+)\s*(?:<[^>]*>)?\s*\(')
RET_F64_BODY_RE = re.compile(r"\s*->\s*f64\s*(?:where[^{;]*)?\{")


def fn_defs(text: str):
    """Yield (name, params_text, start) for every `extern "C" fn` DEFINITION
    returning `f64`; the parameter list may nest parentheses (a macro's
    `$($a: f64),*`)."""
    for m in FN_HEAD_RE.finditer(text):
        depth, i = 1, m.end()
        while i < len(text) and depth:
            depth += {"(": 1, ")": -1}.get(text[i], 0)
            i += 1
        if depth == 0 and RET_F64_BODY_RE.match(text, i):
            yield m.group(1), text[m.end() : i - 1], m.start()


CLOSURE_PARAM_RE = re.compile(
    r"^\*(?:const|mut)\s+(?:[\w:]*::)?(?:ClosureHeader|RawClosureHeader)$|^ClosurePtr$"
)
JS_THIS_RE = re.compile(r"^(?:[\w:]*::)?JsThis$")
ALLOC_RE = re.compile(
    r"\b(?:js_closure_alloc(?:_singleton|_with_captures_singleton)?|js_register_closure_\w+"
    r"|alloc_closure|register_closure_arity)\s*\(\s*(\w+)\s+as\s+\*const",
    re.S,
)


def call_entries(root: Path) -> set[str]:
    """`perry_abi::JS_CALL_ENTRIES`: runtime entry points that take a closure
    as an ordinary argument (and read the receiver from the cell)."""
    text = (root / "crates/perry-abi/src/lib.rs").read_text(encoding="utf-8")
    m = re.search(r"pub const JS_CALL_ENTRIES:[^=]*=\s*\[(.*?)\];", text, re.S)
    if not m:
        raise SystemExit("check_js_body_call_funnel: perry_abi::JS_CALL_ENTRIES not found")
    return set(re.findall(r'"(\w+)"', m.group(1)))


def param_types(params: str) -> list[str]:
    """Types of a comma-separated parameter list (macro repetitions such as
    `$($a: f64),*` count as `f64`)."""
    params = re.sub(r"\$\(|\)\s*,?\s*[*+]", ",", params)
    out = []
    for p in (x.strip() for x in params.split(",")):
        if not p:
            continue
        out.append(p.split(":", 1)[-1].strip() if ":" in p else p)
    return out


def definition_violations(root: Path):
    entries = call_entries(root)
    bases = list(RUNTIME_ROOTS) + sorted(
        p.relative_to(root).as_posix() for p in root.glob("crates/perry-ext-*/src")
    ) + ["crates/perry-ffi/src"]
    defs: dict[str, list[tuple[str, int, list[str]]]] = {}
    texts = []
    out = []
    for base in bases:
        if not (root / base).exists():
            continue
        for path in sorted((root / base).rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            text = path.read_text(encoding="utf-8")
            lines = text.split("\n")
            texts.append((rel, text))
            for name, params, start in fn_defs(text):
                types = param_types(params)
                line = text.count("\n", 0, start) + 1
                defs.setdefault(name, []).append((rel, line, types))
                if not types or not CLOSURE_PARAM_RE.match(types[0]):
                    continue
                if len(types) > 1 and JS_THIS_RE.match(types[1]):
                    continue
                if not all(t == "f64" for t in types[1:]):
                    continue
                if name in entries or exempt(lines, line):
                    continue
                out.append(
                    f"{rel}:{line}: JS body `{name}` does not declare the receiver "
                    "(`_this: JsThis` after the closure; perry_abi::JS_BODY_THIS_PARAM)"
                )
    for rel, text in texts:
        for m in ALLOC_RE.finditer(text):
            name = m.group(1)
            found = defs.get(name)
            if not found:
                continue  # a typed local; the compiler checked its type
            if any(len(t) > 1 and JS_THIS_RE.match(t[1]) for _, _, t in found):
                continue
            line = text.count("\n", 0, m.start()) + 1
            out.append(
                f"{rel}:{line}: `{name}` is installed as a JS body but does not declare "
                "the callee and the receiver (perry_abi::JS_BODY_*)"
            )
    return out


def exempt(lines, line: int) -> bool:
    """A native (non-JS) callback with a body-shaped type — a Rust helper
    another crate registers — is exempt when one of the three lines above the
    transmute says `NOT-A-JS-BODY: <why>`."""
    return any(NOT_A_BODY in lines[i] for i in range(max(0, line - 4), line))


def runtime_violations(root: Path):
    out = []
    bases = list(RUNTIME_ROOTS) + sorted(
        p.relative_to(root).as_posix() for p in root.glob("crates/perry-ext-*/src")
    )
    for base in bases:
        for path in sorted((root / base).rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if rel == RUNTIME_FUNNEL:
                continue
            text = path.read_text(encoding="utf-8")
            lines = text.split("\n")
            for line, target in transmute_targets(text):
                if BODY_TYPE_RE.search(target) and not exempt(lines, line):
                    out.append(f"{rel}:{line}: transmute to a JS body type outside {RUNTIME_FUNNEL}")
    return out


INDIRECT_RE = re.compile(r"\.call_indirect(?:_gc_leaf)?\s*\(")


def codegen_violations(root: Path):
    out = []
    for path in sorted((root / CODEGEN_ROOT).rglob("*.rs")):
        rel = path.relative_to(root).as_posix()
        if rel in CODEGEN_ALLOWED or rel.endswith("_tests.rs"):
            continue
        text = path.read_text(encoding="utf-8")
        for m in INDIRECT_RE.finditer(text):
            line = text.count("\n", 0, m.start()) + 1
            out.append(f"{rel}:{line}: indirect call built outside crates/perry-codegen/src/expr/body_call.rs")
    return out


def self_test() -> int:
    import tempfile

    cases = {
        # (file, text, expect_red)
        "rt_let": ("crates/perry-runtime/src/x.rs",
                   'fn f(p: *const u8) { let g: extern "C" fn(*const ClosureHeader, f64) -> f64 = unsafe { std::mem::transmute(p) }; }', True),
        "rt_turbofish": ("crates/perry-runtime/src/x.rs",
                         'fn f(p: usize) { let _ = unsafe { std::mem::transmute::<usize, extern "C" fn(f64, f64) -> f64>(p) }; }', True),
        "rt_alias": ("crates/perry-runtime/src/x.rs",
                     'type Cb = unsafe extern "C" fn(*const ClosureHeader) -> f64;\nfn f(p: usize) { let c: Cb = std::mem::transmute(p); }', True),
        "rt_erase_ok": ("crates/perry-runtime/src/x.rs",
                        'fn f() { let _ = unsafe { std::mem::transmute::<extern "C" fn(*const ClosureHeader, f64) -> f64, extern "C" fn()>(g) }; }', False),
        "rt_foreign_ok": ("crates/perry-runtime/src/x.rs",
                          'fn f(p: usize) { let g: extern "C" fn(f64) -> i64 = unsafe { std::mem::transmute(p) }; }', False),
        "rt_exempt_ok": ("crates/perry-runtime/src/x.rs",
                         'fn f(p: usize) {\n    // NOT-A-JS-BODY: a registered Rust helper.\n    let g: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(p) };\n}', False),
        "rt_static_getter": ("crates/perry-runtime/src/x.rs",
                             'fn f(p: usize) { let g: extern "C" fn() -> f64 = std::mem::transmute(p); }', True),
        "rt_funnel_ok": (RUNTIME_FUNNEL,
                         'fn f(p: *const u8) { let g: extern "C" fn(f64) -> f64 = unsafe { std::mem::transmute(p) }; }', False),
        "cg_indirect": ("crates/perry-codegen/src/lower_call/x.rs", "fn f() { blk.call_indirect(DOUBLE, &p, &a); }", True),
        "def_no_this": ("crates/perry-runtime/src/x.rs",
                        'extern "C" fn body(_c: *const ClosureHeader, a: f64) -> f64 { a }', True),
        "def_with_this": ("crates/perry-runtime/src/x.rs",
                          'extern "C" fn body(_c: *const ClosureHeader, _this: crate::closure::JsThis, a: f64) -> f64 { a }', False),
        "def_macro_no_this": ("crates/perry-runtime/src/x.rs",
                              'macro_rules! m { ($n:ident) => { extern "C" fn $n(_c: *const ClosureHeader, $($a: f64),*) -> f64 { 0.0 } }; }', True),
        "def_entry_ok": ("crates/perry-runtime/src/x.rs",
                         'pub extern "C" fn js_closure_call2(c: *const ClosureHeader, a: f64, b: f64) -> f64 { a }', False),
        "def_exempt_ok": ("crates/perry-runtime/src/x.rs",
                          '// NOT-A-JS-BODY: a helper taking a closure argument.\nextern "C" fn helper(c: *const ClosureHeader, a: f64) -> f64 { a }', False),
        "def_macro_rep_no_this": ("crates/perry-runtime/src/x.rs",
                                  'macro_rules! m { ($n:ident) => { extern "C" fn $n(c: *const ClosureHeader $(, $a: f64)*) -> f64 { 0.0 } }; }', True),
        "def_macro_rep_ok": ("crates/perry-runtime/src/x.rs",
                             'macro_rules! m { ($n:ident) => { extern "C" fn $n(c: *const ClosureHeader, _this: JsThis $(, $a: f64)*) -> f64 { 0.0 } }; }', False),
        "def_ffi_no_this": ("crates/perry-ext-zz/src/x.rs",
                            'extern "C" fn cb(_c: *const RawClosureHeader, a: f64) -> f64 { a }', True),
        "alloc_receiverless": ("crates/perry-runtime/src/x.rs",
                               'extern "C" fn noop() -> f64 { 0.0 }\nfn f() { js_closure_alloc(noop as *const u8, 0); }', True),
        "alloc_ok": ("crates/perry-runtime/src/x.rs",
                     'extern "C" fn b(_c: *const ClosureHeader, _this: JsThis) -> f64 { 0.0 }\nfn f() { js_closure_alloc(b as *const u8, 0); }', False),
        "cg_funnel_ok": ("crates/perry-codegen/src/expr/body_call.rs", "fn f() { blk.call_indirect(DOUBLE, &p, &a); }", False),
    }
    failed = 0
    for name, (rel, text, expect_red) in cases.items():
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for d in (*RUNTIME_ROOTS, CODEGEN_ROOT, "crates/perry-abi/src"):
                (root / d).mkdir(parents=True, exist_ok=True)
            (root / "crates/perry-abi/src/lib.rs").write_text(
                'pub const JS_CALL_ENTRIES: [&str; 1] = ["js_closure_call2"];', encoding="utf-8"
            )
            p = root / rel
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(text, encoding="utf-8")
            red = bool(
                runtime_violations(root) or codegen_violations(root) or definition_violations(root)
            )
            if red != expect_red:
                print(f"self-test {name}: expected {'red' if expect_red else 'green'}, got {'red' if red else 'green'}")
                failed += 1
    if failed:
        return 1
    print(f"check_js_body_call_funnel self-test: {len(cases)} cases OK")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    violations = runtime_violations(REPO) + codegen_violations(REPO) + definition_violations(REPO)
    if violations:
        print("\n".join(violations))
        print(f"\n{len(violations)} violation(s): JS body calls outside the funnels "
              "(runtime: closure/body_call.rs macros; codegen: expr::body_call::emit_js_body_call) "
              "or JS body definitions without the receiver")
        return 1
    print(
        "check_js_body_call_funnel: every JS body call goes through its funnel "
        "and every JS body declares the receiver"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
