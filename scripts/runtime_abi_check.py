#!/usr/bin/env python3
"""Compare codegen's runtime declarations with the runtime's real signatures.

Generated code calls into perry-runtime (and perry-stdlib, the ext crates, the
UI crates) through `module.declare_function("name", RET, &[PARAMS])` in
perry-codegen. Nothing ties those declarations to the `#[no_mangle] extern "C"`
definitions they name, and a mismatch is silent: the call compiles and returns
garbage (runtime_decls/mod.rs: "Mismatch is silent and deadly").

This script reads both sides as source text and classifies every parameter and
return by ABI class, then reports each disagreement in one category:

  on every target (real bugs today)
    class          float vs integer/pointer (different registers)
    width          integer widths disagree (i32 vs i64, bool read as i32, a
                   pointer passed as i32, ...)
    arity          parameter counts disagree
    ret_missing    codegen reads a return value the runtime never produces

  on wasm32 only (the #11378 worklist)
    ptrw_as_i64    runtime takes/returns a pointer or usize, codegen says i64
                   (right on LP64, wrong wherever pointers are 32-bit)
    i64_as_ptr     runtime takes/returns a 64-bit integer, codegen says ptr
    stub_signature the only definition is a link stub whose signature differs
                   from the declaration. Harmless on native C ABIs (the stub
                   ignores its arguments and its result is a dummy), but wasm
                   requires every call to match its callee's type exactly.

    ret_ignored    codegen declares void over a value-returning function
                   (the result is simply dropped on native targets)

  bookkeeping
    unresolved     no `extern "C"` definition found (C source, macro-generated,
                   or a dead declaration)
    unclassified   a runtime type this script cannot reduce to an ABI class
    decl_conflict  two codegen call sites declare one symbol differently.
                   `declare_function` keeps whichever runs first in a module,
                   so which shape a call gets depends on emission order; every
                   distinct shape is also compared against the runtime.
    def_conflict   two runtime crates define one symbol with different ABIs

Usage:
  scripts/runtime_abi_check.py                  # summary
  scripts/runtime_abi_check.py --list width     # entries in one category
  scripts/runtime_abi_check.py --json out.json  # full report
  scripts/runtime_abi_check.py --self-test
"""

from __future__ import annotations

import argparse
import json
import pathlib
import re
import sys
from collections import defaultdict
from dataclasses import dataclass, field

ROOT = pathlib.Path(__file__).resolve().parent.parent
CODEGEN_SRC = ROOT / "crates/perry-codegen/src"
CRATES = ROOT / "crates"

EVERY_TARGET = ("class", "width", "arity", "ret_missing")
ILP32_ONLY = ("ptrw_as_i64", "i64_as_ptr", "stub_signature", "ret_ignored")
BOOKKEEPING = (
    "unresolved",
    "unclassified",
    "decl_conflict",
    "def_conflict",
)
CATEGORIES = EVERY_TARGET + ILP32_ONLY + BOOKKEEPING

# ---------------------------------------------------------------------------
# ABI classes
# ---------------------------------------------------------------------------
F64, F32, I64, I32, I16, I8, BOOL, PTRW, VOID, NEVER, PTR = (
    "f64", "f32", "i64", "i32", "i16", "i8", "bool", "ptrw", "void", "never", "ptr",
)

CODEGEN_TYPES = {
    "DOUBLE": F64, "double": F64,
    "F32": F32, "float": F32,
    "I64": I64, "i64": I64,
    "I32": I32, "i32": I32,
    "I16": I16, "i16": I16,
    "I8": I8, "i8": I8,
    "I1": BOOL, "i1": BOOL,
    "PTR": PTR, "ptr": PTR,
    "VOID": VOID, "void": VOID,
}

RUST_SCALARS = {
    "f64": F64, "c_double": F64,
    "f32": F32, "c_float": F32,
    "i64": I64, "u64": I64, "c_longlong": I64, "c_ulonglong": I64,
    "i32": I32, "u32": I32, "c_int": I32, "c_uint": I32, "char": I32,
    "i16": I16, "u16": I16, "c_short": I16, "c_ushort": I16,
    "i8": I8, "u8": I8, "c_char": I8, "c_schar": I8, "c_uchar": I8,
    "bool": BOOL,
    # Pointer-width integers. `c_long` is 64-bit on LP64 Unix and 32-bit on
    # wasm32, i.e. it tracks the pointer width on every target this matters for.
    "usize": PTRW, "isize": PTRW, "size_t": PTRW, "ssize_t": PTRW,
    "uintptr_t": PTRW, "intptr_t": PTRW, "c_long": PTRW, "c_ulong": PTRW,
    "()": VOID, "!": NEVER,
}


def split_top(text: str, sep: str = ",") -> list[str]:
    """Split on `sep` outside (), [], <>, {} and string literals."""
    out, depth, cur, i, in_str = [], 0, [], 0, False
    while i < len(text):
        ch = text[i]
        if in_str:
            cur.append(ch)
            if ch == "\\" and i + 1 < len(text):
                cur.append(text[i + 1])
                i += 2
                continue
            if ch == '"':
                in_str = False
        elif ch == '"':
            in_str = True
            cur.append(ch)
        elif ch in "([{<":
            depth += 1
            cur.append(ch)
        elif ch in ")]}>":
            # `->` inside a fn-pointer type is not a closing bracket.
            if ch == ">" and i > 0 and text[i - 1] == "-":
                cur.append(ch)
            else:
                depth -= 1
                cur.append(ch)
        elif ch == sep and depth == 0:
            out.append("".join(cur).strip())
            cur = []
        else:
            cur.append(ch)
        i += 1
    tail = "".join(cur).strip()
    if tail:
        out.append(tail)
    return out


def balanced(text: str, start: int, open_: str = "(", close: str = ")") -> int:
    """Index just past the bracket matching text[start] == open_."""
    depth, i, in_str = 0, start, False
    while i < len(text):
        ch = text[i]
        if in_str:
            if ch == "\\":
                i += 2
                continue
            if ch == '"':
                in_str = False
        elif ch == '"':
            in_str = True
        elif ch == open_:
            depth += 1
        elif ch == close:
            depth -= 1
            if depth == 0:
                return i + 1
        i += 1
    return -1


def strip_comments(text: str) -> str:
    """Lex Rust source once and blank everything that is not code or an
    ordinary string literal: comments, char/byte literals, raw strings.
    Offsets and newlines are kept, so line numbers stay exact, and the
    bracket matching below only ever has to understand plain strings."""
    out = list(text)
    n, i = len(text), 0

    def blank(a: int, b: int) -> None:
        for k in range(a, min(b, n)):
            if out[k] != "\n":
                out[k] = " "

    while i < n:
        ch = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            blank(i, j)
            i = j
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            blank(i, j)
            i = j
        elif (m := re.match(r'b?r(#*)"', text[i:i + 70])) and (i == 0 or not (text[i - 1].isalnum() or text[i - 1] == "_")):
            close = '"' + m.group(1)
            j = text.find(close, i + m.end())
            j = n if j < 0 else j + len(close)
            blank(i, j)
            i = j
        elif ch == '"':
            j = i + 1
            while j < n and text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            i = j + 1
        elif ch == "'":
            m = re.match(r"'(?:\\u\{[0-9a-fA-F]+\}|\\.|[^'\\\n])'", text[i:i + 12])
            if m:
                start = i - 1 if i > 0 and text[i - 1] == "b" else i
                blank(start, i + m.end())
                i += m.end()
            else:
                i += 1  # a lifetime
        else:
            i += 1
    return "".join(out)


CFG_TEST_RE = re.compile(r"#\[cfg\(test\)\]")


def strip_test_items(text: str) -> str:
    """Blank `#[cfg(test)]` items (inline test modules, test-only fns): their
    declarations and calls never reach a link. Offsets and newlines are kept."""
    out = list(text)
    for m in CFG_TEST_RE.finditer(text):
        brace = text.find("{", m.end())
        semi = text.find(";", m.end())
        if brace < 0 or (0 <= semi < brace):
            continue  # `#[cfg(test)] use ...;` / `mod x;` — nothing inline
        end = balanced(text, brace, "{", "}")
        if end < 0:
            continue
        for k in range(m.start(), end):
            if out[k] != "\n":
                out[k] = " "
    return "".join(out)


def line_of(text: str, offset: int) -> int:
    return text.count("\n", 0, offset) + 1


# ---------------------------------------------------------------------------
# Runtime side
# ---------------------------------------------------------------------------
@dataclass
class RustFn:
    name: str
    params: list[str]
    ret: str
    where: str


ALIAS_RE = re.compile(r"\btype\s+(\w+)\s*=\s*([^;]+);")
TRANSPARENT_RE = re.compile(
    r"#\[repr\(transparent\)\]\s*(?:#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?struct\s+(\w+)"
    r"\s*(?:\(\s*(?:pub(?:\([^)]*\))?\s+)?([^)]+?)\s*\)\s*;|\{\s*(?:pub(?:\([^)]*\))?\s+)?\w+\s*:\s*([^,}]+?)\s*,?\s*\})"
)
EXTERN_FN_RE = re.compile(r'extern\s+"C(?:-unwind)?"\s+fn\s+(\w+)\s*\(')
EXPORT_NAME_RE = re.compile(r'#\[(?:unsafe\()?export_name\s*=\s*"([^"]+)"')


def codegen_sources() -> list[pathlib.Path]:
    # Codegen's own unit tests declare throwaway symbols; they never reach a link.
    return sorted(
        p
        for p in CODEGEN_SRC.rglob("*.rs")
        if not (p.name == "tests.rs" or p.name.endswith("_tests.rs") or "/tests/" in str(p))
    )


def rust_sources() -> list[pathlib.Path]:
    return sorted(
        p
        for p in CRATES.rglob("*.rs")
        if "/target/" not in str(p) and "/tests/fixtures/" not in str(p)
    )


def parse_rust(files: dict[str, str]) -> tuple[dict[str, list[RustFn]], dict[str, set[str]]]:
    defs: dict[str, list[RustFn]] = defaultdict(list)
    aliases: dict[str, set[str]] = defaultdict(set)
    for path, raw in files.items():
        text = strip_test_items(strip_comments(raw))
        for m in ALIAS_RE.finditer(text):
            aliases[m.group(1)].add(" ".join(m.group(2).split()))
        for m in TRANSPARENT_RE.finditer(text):
            inner = m.group(2) or m.group(3)
            if inner:
                aliases[m.group(1)].add(" ".join(inner.split()))
        for m in EXTERN_FN_RE.finditer(text):
            # Attributes: the text between the previous item boundary and here.
            head_start = max(text.rfind("}", 0, m.start()), text.rfind(";", 0, m.start()))
            head = text[head_start + 1 : m.start()]
            export = EXPORT_NAME_RE.search(head)
            if not export and "no_mangle" not in head:
                continue
            name = export.group(1) if export else m.group(1)
            open_paren = m.end() - 1
            close = balanced(text, open_paren)
            if close < 0:
                continue
            params = []
            for p in split_top(text[open_paren + 1 : close - 1]):
                if ":" in p:
                    params.append(" ".join(p.split(":", 1)[1].split()))
            rest = text[close : close + 400]
            rm = re.match(r"\s*->\s*(.+?)\s*(?:\{|where\b|;)", rest, re.S)
            ret = " ".join(rm.group(1).split()) if rm else "()"
            defs[name].append(RustFn(name, params, ret, f"{path}:{line_of(text, m.start())}"))
    return defs, aliases


PTR_PREFIX_RE = re.compile(r"^(\*\s*(mut|const)\b|&)")
FN_PTR_RE = re.compile(r'^(unsafe\s+)?(extern\s+"[^"]+"\s+)?fn\s*\(')


def classify(ty: str, aliases: dict[str, set[str]], depth: int = 0) -> str:
    t = " ".join(ty.split())
    if depth > 8:
        return f"?{t}"
    if t in RUST_SCALARS:
        return RUST_SCALARS[t]
    if PTR_PREFIX_RE.match(t) or FN_PTR_RE.match(t):
        return PTRW
    m = re.match(r"^(?:[\w:]+::)?(Option|NonNull|Box)\s*<(.+)>$", t)
    if m:
        if m.group(1) in ("NonNull", "Box"):
            return PTRW
        inner = classify(m.group(2), aliases, depth + 1)
        # Option<&T> / Option<NonNull<T>> / Option<fn> are pointer-sized by the
        # null-pointer optimization; any other Option is not an FFI scalar.
        return PTRW if inner == PTRW else f"?{t}"
    last = t.split("::")[-1]
    if last in RUST_SCALARS:
        return RUST_SCALARS[last]
    targets = aliases.get(last)
    if targets:
        classes = {classify(x, aliases, depth + 1) for x in targets}
        if len(classes) == 1:
            return classes.pop()
    return f"?{t}"


# ---------------------------------------------------------------------------
# Codegen side
# ---------------------------------------------------------------------------
@dataclass
class Decl:
    name: str
    params: list[str]
    ret: str
    where: str
    origin: str = "declare"


DECL_RE = re.compile(r"\.declare_function(?:_with_ret_attrs)?\s*\(")
PENDING_RE = re.compile(r"pending_declares\s*\.push\s*\(\s*\(")
CALL_RE = re.compile(r"\.call(_void)?\s*\(")
NATIVE_SIG_RE = re.compile(r"NativeModSig\s*\{")
NAME_LIT = re.compile(r'"([\w.$]+)"')

# `NativeModSig` kinds, as `lower_call/native_module_dispatch.rs` lowers them.
NATIVE_ARG = {"NA_F64": F64, "NA_STR": I64, "NA_PTR": I64, "NA_JSV": I64, "NA_VARARGS": I64}
NATIVE_RET = {
    "NR_GCPTR": I64, "NR_NULLABLE_GCPTR": I64, "NR_HANDLE_ID": I64,
    "NR_FOREIGN_PTR": I64, "NR_JS_VALUE": I64, "NR_PROMISE": I64, "NR_STR": I64,
    "NR_OBJ_FROM_JSON_STR": I64, "NR_BIGINT": I64, "NR_F64": F64, "NR_BOOL": F64,
    "NR_I32": I32, "NR_VOID": VOID,
}


def codegen_type(expr: str) -> str:
    e = expr.strip().strip('"').split("::")[-1]
    return CODEGEN_TYPES.get(e, f"?{expr.strip()}")


def type_list(text: str, at: int, expr: str) -> list[str] | None:
    """Types of `&[A, B]` / `vec![A, B]`, or of a `let x = &[..]` bound earlier."""
    expr = expr.strip()
    m = re.fullmatch(r"(?:&\s*|vec!\s*)\[(.*)\]", expr, re.S)
    if m:
        return [codegen_type(p) for p in split_top(m.group(1))]
    if re.fullmatch(r"\w+", expr):
        lets = list(re.finditer(rf"let\s+{expr}\s*(?::[^=]+)?=\s*(&\s*\[[^\]]*\])\s*;", text[max(0, at - 3000):at]))
        if lets:
            return type_list(text, at, lets[-1].group(1))
    return None


def names_for(text: str, at: int, expr: str) -> list[str] | None:
    """A literal name, or the literals of an enclosing `for f in [..]` loop."""
    expr = expr.strip()
    m = re.fullmatch(r'"([\w.$]+)"', expr)
    if m:
        return [m.group(1)]
    if re.fullmatch(r"\w+", expr):
        loops = list(re.finditer(rf"for\s+&?{expr}\s+in\s+&?\[([^\]]*)\]", text[max(0, at - 4000):at]))
        if loops:
            names = NAME_LIT.findall(loops[-1].group(1))
            if names:
                return names
    return None


def parse_codegen(files: dict[str, str]) -> tuple[list[Decl], dict[str, int]]:
    decls: list[Decl] = []
    skipped: dict[str, int] = defaultdict(int)

    def add(origin, names, params, ret, where):
        for n in names:
            decls.append(Decl(n, params, ret, where, origin))

    for path, raw in files.items():
        text = strip_test_items(strip_comments(raw))
        for m in DECL_RE.finditer(text):
            o = m.end() - 1
            c = balanced(text, o)
            args = split_top(text[o + 1 : c - 1]) if c > 0 else []
            names = names_for(text, m.start(), args[0]) if len(args) >= 3 else None
            params = type_list(text, m.start(), args[2]) if len(args) >= 3 else None
            if not names or params is None:
                skipped["declare"] += 1
                continue
            add("declare", names, params, codegen_type(args[1]), f"{path}:{line_of(text, m.start())}")
        for m in PENDING_RE.finditer(text):
            o = m.end() - 1
            c = balanced(text, o)
            args = split_top(text[o + 1 : c - 1]) if c > 0 else []
            if len(args) < 3:
                skipped["pending_declare"] += 1
                continue
            nm = re.fullmatch(r'"([\w.$]+)"(?:\s*\.\s*to_string\(\)|\s*\.\s*into\(\))?', args[0].strip())
            params = type_list(text, m.start(), args[2])
            if not nm or params is None:
                skipped["pending_declare"] += 1
                continue
            add("pending_declare", [nm.group(1)], params, codegen_type(args[1]), f"{path}:{line_of(text, m.start())}")
        for m in CALL_RE.finditer(text):
            o = m.end() - 1
            c = balanced(text, o)
            args = split_top(text[o + 1 : c - 1]) if c > 0 else []
            void = m.group(1) is not None
            if void:
                if len(args) != 2:
                    skipped["call"] += 1
                    continue
                ret, name_expr, arg_expr = VOID, args[0], args[1]
            else:
                if len(args) != 3:
                    skipped["call"] += 1
                    continue
                ret, name_expr, arg_expr = codegen_type(args[0]), args[1], args[2]
            nm = re.fullmatch(r'"([\w.$]+)"', name_expr.strip())
            am = re.fullmatch(r"&\s*\[(.*)\]", arg_expr.strip(), re.S)
            if not nm or not am:
                skipped["call"] += 1
                continue
            params = []
            for tup in split_top(am.group(1)):
                tm = re.fullmatch(r"\(\s*(.*)\)", tup.strip(), re.S)
                parts = split_top(tm.group(1)) if tm else []
                params.append(codegen_type(parts[0]) if parts else f"?{tup}")
            add("call", [nm.group(1)], params, ret, f"{path}:{line_of(text, m.start())}")
        for m in NATIVE_SIG_RE.finditer(text):
            c = balanced(text, m.end() - 1, "{", "}")
            body = text[m.end() : c - 1] if c > 0 else ""
            rt = re.search(r'runtime:\s*"(\w+)"', body)
            ar = re.search(r"args:\s*&\s*\[([^\]]*)\]", body)
            rr = re.search(r"ret:\s*(\w+)", body)
            recv = re.search(r"has_receiver:\s*(true|false)", body)
            if not (rt and ar and rr and recv):
                skipped["native_table"] += 1
                continue
            params = [NATIVE_ARG.get(a.strip(), f"?{a.strip()}") for a in split_top(ar.group(1))]
            if recv.group(1) == "true":
                params = [I64] + params
            add(
                "native_table", [rt.group(1)], params,
                NATIVE_RET.get(rr.group(1), f"?{rr.group(1)}"), f"{path}:{line_of(text, m.start())}",
            )
    return decls, skipped


# ---------------------------------------------------------------------------
# Comparison
# ---------------------------------------------------------------------------
def compare_slot(cg: str, rt: str, is_ret: bool) -> str | None:
    """Category for one parameter/return pair, or None when they agree."""
    if rt.startswith("?") or cg.startswith("?"):
        return "unclassified"
    if rt == NEVER:
        rt = VOID
    if is_ret and cg == VOID and rt != VOID:
        return "ret_ignored"
    if is_ret and cg != VOID and rt == VOID:
        return "ret_missing"
    if cg == rt:
        return None
    if cg == PTR and rt == PTRW:
        return None
    if cg == I64 and rt == PTRW:
        return "ptrw_as_i64"
    if cg == PTR and rt == I64:
        return "i64_as_ptr"
    floats = {F64, F32}
    if (cg in floats) != (rt in floats) or (cg in floats and rt in floats):
        return "class"
    # bool travels as i1 or i8 on every C ABI; anything wider reads undefined bits.
    if rt == BOOL and cg in (I8, BOOL):
        return None
    return "width"


@dataclass
class Report:
    entries: dict[str, list[dict]] = field(default_factory=lambda: defaultdict(list))
    declared: int = 0
    skipped: dict[str, int] = field(default_factory=dict)

    def add(self, cat: str, **info) -> None:
        self.entries[cat].append(info)


def definition_rank(where: str) -> tuple[int, int]:
    path = where.split(":", 1)[0]
    stub = 1 if "stub" in pathlib.PurePosixPath(path).name else 0
    order = ("perry-runtime/", "perry-stdlib/", "perry-ext-")
    crate = next((i for i, c in enumerate(order) if f"crates/{c}" in path), len(order))
    return (stub, crate)


def judge(rep: "Report", name: str, d: "Decl", shapes: dict) -> None:
    """Compare one declared shape of `name` against its runtime definitions."""
    # Several crates may define the symbol (a real implementation plus
    # link-only stubs for hosts that lack the feature). Any definition that
    # agrees is the one generated code links against in that build;
    # otherwise judge against the real one: non-stub first, runtime crates
    # before UI/host crates.
    if any(
        len(sp[0]) == len(d.params)
        and compare_slot(d.ret, sp[1], True) is None
        and all(compare_slot(c, r, False) is None for c, r in zip(d.params, sp[0]))
        for sp in shapes
    ):
        return
    (rparams, rret), rf = min(shapes.items(), key=lambda kv: definition_rank(kv[1].where))
    base = dict(symbol=name, origin=d.origin, codegen=d.where, runtime=rf.where)
    if definition_rank(rf.where)[0] == 1:
        rep.add(
            "stub_signature",
            **base,
            detail=f"codegen {d.ret}({', '.join(d.params)}), "
            f"stub {rret}({', '.join(rparams)})",
        )
        return
    if len(d.params) != len(rparams):
        rep.add(
            "arity",
            **base,
            detail=f"codegen {len(d.params)} params, runtime {len(rparams)}",
        )
        return
    seen = set()
    cat = compare_slot(d.ret, rret, True)
    if cat:
        seen.add(cat)
        rep.add(cat, **base, detail=f"return: codegen {d.ret}, runtime {rf.ret} ({rret})")
    for i, (cg, rt) in enumerate(zip(d.params, rparams)):
        cat = compare_slot(cg, rt, False)
        if cat and cat not in seen:
            seen.add(cat)
            rep.add(cat, **base, detail=f"param {i}: codegen {cg}, runtime {rf.params[i]} ({rt})")


def check(codegen_files: dict[str, str], rust_files: dict[str, str]) -> Report:
    decls, skipped = parse_codegen(codegen_files)
    defs, aliases = parse_rust(rust_files)
    rep = Report(skipped=dict(skipped))

    by_name: dict[str, list[Decl]] = defaultdict(list)
    for d in decls:
        by_name[d.name].append(d)
    rep.declared = len(by_name)

    for name, ds in sorted(by_name.items()):
        if name.startswith("llvm."):
            continue  # LLVM intrinsics: lowered by the backend, not linked
        decl_like = [d for d in ds if d.origin in ("declare", "pending_declare")]
        first = decl_like[0] if decl_like else ds[0]
        for other in decl_like[1:]:
            if (other.params, other.ret) != (first.params, first.ret):
                rep.add(
                    "decl_conflict",
                    symbol=name,
                    first=f"{first.ret}({', '.join(first.params)}) @ {first.where}",
                    other=f"{other.ret}({', '.join(other.params)}) @ {other.where}",
                )
        rfs = defs.get(name)
        if not rfs:
            rep.add("unresolved", symbol=name, codegen=first.where)
            continue
        shapes = {
            (tuple(classify(p, aliases) for p in rf.params), classify(rf.ret, aliases)): rf
            for rf in rfs
        }
        if len(shapes) > 1:
            rep.add(
                "def_conflict",
                symbol=name,
                definitions=[f"{s[1]}({', '.join(s[0])}) @ {rf.where}" for s, rf in shapes.items()],
            )
        # Every distinct shape is live somewhere: whichever declaration runs
        # first in a module wins, and each call instruction carries its own
        # function type (opaque pointers), so each one is judged.
        shapes_seen = []
        for d in ds:
            if (d.params, d.ret) in shapes_seen:
                continue
            shapes_seen.append((d.params, d.ret))
            judge(rep, name, d, shapes)
    return rep


def load(paths) -> dict[str, str]:
    out = {}
    for p in paths:
        try:
            out[str(p.relative_to(ROOT))] = p.read_text(errors="replace")
        except OSError:
            pass
    return out


def summary(rep: Report) -> str:
    lines = [
        f"runtime_abi_check: {rep.declared} symbols named by codegen "
        f"(declarations, call sites, pending declares, native tables)",
        "  not checkable statically (non-literal name or types): "
        + ", ".join(f"{k} {v}" for k, v in sorted(rep.skipped.items())),
    ]
    for title, cats in (
        ("wrong on every target", EVERY_TARGET),
        ("wrong on wasm32 only", ILP32_ONLY),
        ("bookkeeping", BOOKKEEPING),
    ):
        lines.append(f"  {title}:")
        for c in cats:
            lines.append(f"    {c:<14} {len(rep.entries.get(c, [])):>5}")
    return "\n".join(lines)


# ---------------------------------------------------------------------------
# Self-test: every category must be reachable, and agreement must be silent.
# ---------------------------------------------------------------------------
def self_test() -> int:
    codegen = {
        "cg.rs": """
        fn d(m: &mut LlModule) {
            m.declare_function("ok_ptr", PTR, &[I64, DOUBLE]);
            m.declare_function("ok_alias", DOUBLE, &[PTR]);
            m.declare_function("class_bug", I64, &[I64]);
            m.declare_function("width_bug", I32, &[]);
            m.declare_function("arity_bug", VOID, &[I64]);
            m.declare_function("missing_ret", I32, &[]);
            m.declare_function("ignored_ret", VOID, &[]);
            m.declare_function("ptrw", I64, &[]);
            m.declare_function("i64ptr", VOID, &[PTR]);
            m.declare_function("nowhere", VOID, &[]);
            m.declare_function("odd", VOID, &[PTR]);
            m.declare_function("twice", VOID, &[I64]);
            m.declare_function("twice", VOID, &[I32]);
            m.declare_function("bool_ok", I8, &[]);
            m.declare_function("stubbed", I64, &[I64, I64]);
            m.declare_function(name, VOID, &[]);
            // m.declare_function("commented", VOID, &[]);
            for f in ["loop_a", "loop_b"] {
                m.declare_function(f, DOUBLE, &[DOUBLE]);
            }
            let shared = &[DOUBLE, DOUBLE];
            m.declare_function("let_bound", DOUBLE, shared);
            ctx.pending_declares.push(("pend".to_string(), I64, vec![I64]));
            let r = blk.call(I64, "call_site", &[(DOUBLE, &x)]);
            blk.call_void("call_void_site", &[(I64, &h)]);
            blk.call(I64, dynamic_name, &args);
        }
        #[cfg(test)]
        mod tests {
            fn t(b: &mut Block) { let q = b'"'; let r = r#"a "} b"#; b.call_void("width_bug", &[(I64, "0")]); }
            fn u<'a>(x: &'a str) {}
        }
        const T: &[NativeModSig] = &[NativeModSig {
            module: "m", has_receiver: true, method: "x", class_filter: None,
            runtime: "native_row", args: &[NA_F64], ret: NR_BOOL,
        }];
        """,
    }
    rust = {
        "rt.rs": """
        #[repr(transparent)]
        pub struct JSValue { bits: u64 }
        pub type Handle = *mut u8;
        #[no_mangle] pub extern "C" fn ok_ptr(a: i64, b: f64) -> *mut Foo { todo!() }
        #[no_mangle] pub extern "C" fn ok_alias(h: Handle) -> f64 { 0.0 }
        #[no_mangle] pub extern "C" fn class_bug(v: JSValue) -> f64 { 0.0 }
        #[no_mangle] pub extern "C" fn width_bug() -> i64 { 0 }
        #[no_mangle] pub extern "C" fn arity_bug(a: i64, b: i64) {}
        #[no_mangle] pub extern "C" fn missing_ret() {}
        #[no_mangle] pub extern "C" fn ignored_ret() -> u32 { 0 }
        #[no_mangle] pub extern "C" fn ptrw() -> usize { 0 }
        #[unsafe(no_mangle)] pub unsafe extern "C" fn i64ptr(v: u64) {}
        #[no_mangle] pub extern "C" fn odd(v: SomeStruct) {}
        #[no_mangle] pub extern "C" fn twice(v: i64) {}
        #[no_mangle] pub extern "C" fn bool_ok() -> bool { true }
        #[no_mangle] pub extern "C" fn loop_a(v: f64) -> f64 { v }
        #[no_mangle] pub extern "C" fn loop_b(v: i64) -> f64 { 0.0 }
        #[no_mangle] pub extern "C" fn let_bound(a: f64, b: f64) -> f64 { a }
        #[no_mangle] pub extern "C" fn pend(v: *mut u8) -> usize { 0 }
        #[no_mangle] pub extern "C" fn call_site(v: f64) -> i64 { 0 }
        #[no_mangle] pub extern "C" fn call_void_site(v: i64) -> u32 { 0 }
        #[no_mangle] pub extern "C" fn native_row(h: i64, v: f64) -> f64 { 0.0 }
        pub extern "C" fn not_exported(v: f64) {}
        """,
        "my_stubs.rs": """
        #[no_mangle] pub extern "C" fn stubbed() -> f64 { 0.0 }
        """,
        "rt2.rs": """
        #[no_mangle] pub extern "C" fn ignored_ret() -> u64 { 0 }
        """,
    }
    rep = check(codegen, rust)
    expect = {
        "class": {"class_bug", "loop_b"},
        "width": {"width_bug", "twice"},
        "arity": {"arity_bug"},
        "ret_missing": {"missing_ret"},
        "ret_ignored": {"ignored_ret", "call_void_site"},
        "ptrw_as_i64": {"ptrw", "pend"},
        "i64_as_ptr": {"i64ptr"},
        "unresolved": {"nowhere"},
        "unclassified": {"odd"},
        "decl_conflict": {"twice"},
        "def_conflict": {"ignored_ret"},
        "stub_signature": {"stubbed"},
    }
    failures = []
    for cat in CATEGORIES:
        got = {e["symbol"] for e in rep.entries.get(cat, [])}
        if got != expect.get(cat, set()):
            failures.append(f"{cat}: expected {sorted(expect.get(cat, set()))}, got {sorted(got)}")
    if rep.skipped != {"declare": 1, "call": 1}:
        failures.append(f"skipped: expected declare 1 + call 1, got {rep.skipped}")
    if rep.declared != 21:
        failures.append(f"declared: expected 21 symbols, got {rep.declared}")
    origins = {e["symbol"]: e["origin"] for c in CATEGORIES for e in rep.entries.get(c, []) if "origin" in e}
    for sym, origin in (("pend", "pending_declare"), ("call_void_site", "call"), ("loop_b", "declare")):
        if origins.get(sym) != origin:
            failures.append(f"origin of {sym}: expected {origin}, got {origins.get(sym)}")
    if failures:
        print("runtime_abi_check self-test FAILED:", file=sys.stderr)
        for f in failures:
            print(f"  - {f}", file=sys.stderr)
        return 1
    print("runtime_abi_check self-test: OK (every category reachable, agreement silent)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--list", metavar="CATEGORY", choices=CATEGORIES)
    ap.add_argument("--json", metavar="PATH")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    rep = check(load(codegen_sources()), load(rust_sources()))
    print(summary(rep))
    if args.list:
        for e in rep.entries.get(args.list, []):
            print("  " + json.dumps(e))
    if args.json:
        pathlib.Path(args.json).write_text(
            json.dumps({c: rep.entries.get(c, []) for c in CATEGORIES}, indent=1) + "\n"
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
