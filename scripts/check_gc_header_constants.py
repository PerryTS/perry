#!/usr/bin/env python3
"""Hold every compiler-side restatement of a GC header bit to the runtime's own.

WHY THIS EXISTS
---------------
`perry-codegen` does NOT depend on `perry-runtime` — its dependencies are
perry-hir, perry-dispatch and perry-api-manifest. Yet the compiler bakes the
collector's header layout into emitted code in two load-bearing ways:

* the inline `new` path stores a packed `GcHeader` word as a COMPILE-TIME
  constant (`target_layout::inline_alloc_gc_packed`, #8122), pre-composed per
  class into `@perry_class_header_image_*`; and
* every class-field / element-shape guard masks that word against a literal
  and compares it to a literal (`expr/class_field_inline_guard.rs`,
  `expr/element_shape_guard.rs`).

Both sides therefore carry their own copy of `GC_TYPE_OBJECT`,
`GC_FLAG_FORWARDED`, `OBJ_FLAG_HAS_DESCRIPTORS`, `GC_OBJ_TYPED_LAYOUT_INTACT`
and friends, and until this checker the agreement was held by a code comment
("Runtime-side name: `gc::layout::GC_OBJ_TYPED_LAYOUT_INTACT`").

The things that LOOK like they enforce it do not:

    // expr/class_field_inline_guard.rs
    const GC_FLAG_FORWARDED_I8: &str = "-128";
    ...
    debug_assert_eq!(GC_FLAG_FORWARDED_I8, "-128");

That compares codegen's constant to a string literal — a tautology. It is a
useful "you edited the const, now fix the mask arithmetic below" pin, but it
never references the runtime, so it cannot detect a renumbering there; and per
CLAUDE.md's profile note it is compiled out of `release` AND `perry-dev`
anyway. Every codegen test naming these bits asserts codegen's own constant
appears in the emitted IR, so they pin codegen to itself and would all stay
green.

So, before this checker, renumbering a flag in `perry-runtime` compiled clean,
passed every suite, and shipped a compiler whose inline allocator baked one bit
layout while the collector read another — objects born with flags the GC
misreads. CLAUDE.md describes that class of bug as surfacing cycles later as
`TypeError: value is not a function`, nowhere near the cause.

WHAT THIS DOES NOT CATCH (stated plainly, per CLAUDE.md's gate rules)
--------------------------------------------------------------------
* Whether a bit ASSIGNMENT is the right one. This only proves the two sides
  agree, never that the value is well chosen.
* Restatements that are not a `const` declaration — a bare literal inline in
  an expression is invisible here. The registry below is the defence against
  that: a checked constant must be DECLARED, so review has one place to look.
* Layout contracts other than the 32-bit GC header word. String, Map and array
  header offsets/sizes are duplicated the same way and are enumerated in
  `OUT_OF_SCOPE` rather than silently ignored — they belong to different
  subsystems and want their own derivation, not a second-guessing one here.
* Field OFFSETS within `GcHeader` (obj_type @-8, gc_flags @-7, _reserved @-6).
  Those are asserted structurally by the runtime's own layout tests.

Usage:
    python3 scripts/check_gc_header_constants.py             # check
    python3 scripts/check_gc_header_constants.py --list      # describe
    python3 scripts/check_gc_header_constants.py --self-test # prove it can fail
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# ---------------------------------------------------------------------------
# The authoritative side: where each runtime constant is DEFINED.
# ---------------------------------------------------------------------------
RUNTIME_SOURCES = [
    "crates/perry-runtime/src/gc/types.rs",
    "crates/perry-runtime/src/gc/layout.rs",
]

# Runtime constants this checker resolves. Anything a codegen restatement
# derives from must be listed here, so a rename on the runtime side fails loudly
# instead of leaving a restatement unanchored.
RUNTIME_WANTED = {
    "GC_TYPE_ARRAY",
    "GC_TYPE_OBJECT",
    "GC_TYPE_MAP",
    "GC_FLAG_ARENA",
    "GC_FLAG_FORWARDED",
    "OBJ_FLAG_FROZEN",
    "OBJ_FLAG_PACKED_NUMERIC_PROOF",
    "OBJ_FLAG_PLAIN_ORDINARY",
    "OBJ_FLAG_ARRAY_DESCRIPTORS",
    "OBJ_FLAG_STABLE_TOMBSTONES",
    "OBJ_FLAG_HAS_DESCRIPTORS",
    "GC_LAYOUT_POINTER_FREE",
    "GC_LAYOUT_SIDE_MASK",
    "GC_OBJ_TYPED_LAYOUT_INTACT",
}

# ---------------------------------------------------------------------------
# The registry: every codegen-side restatement, and how to re-derive it.
#
# `expr` is evaluated with the runtime constants in scope. A restatement whose
# declaration has vanished FAILS — a fix must delete its entry, so this list
# cannot rot into a description of a tree that no longer exists.
#
# Byte positions inside the 32-bit header word (little-endian):
#   bits  0..7  obj_type | bits 8..15 gc_flags | bits 16..31 _reserved
# ---------------------------------------------------------------------------
Restatement = tuple[str, str, str, str]  # (file, const, expr, why)

REGISTRY: list[Restatement] = [
    # --- the packed GcHeader word the inline `new` path bakes (#8122) --------
    ("crates/perry-codegen/src/target_layout.rs", "GC_TYPE_OBJECT",
     "GC_TYPE_OBJECT", "byte 0 of the baked header word"),
    ("crates/perry-codegen/src/target_layout.rs", "GC_FLAG_ARENA",
     "GC_FLAG_ARENA", "byte 1 of the baked header word"),
    ("crates/perry-codegen/src/target_layout.rs", "GC_LAYOUT_POINTER_FREE",
     "GC_LAYOUT_POINTER_FREE", "_reserved half of the baked header word"),
    ("crates/perry-codegen/src/target_layout.rs", "GC_LAYOUT_SIDE_MASK",
     "GC_LAYOUT_SIDE_MASK", "_reserved half of the baked header word"),
    ("crates/perry-codegen/src/target_layout.rs", "GC_OBJ_TYPED_LAYOUT_INTACT",
     "GC_OBJ_TYPED_LAYOUT_INTACT", "_reserved half of the baked header word"),
    # `new_alloc.rs` re-derives the same word at the allocation site and
    # cross-checks it against the per-class table; both copies are pinned.
    ("crates/perry-codegen/src/lower_call/new_alloc.rs", "GC_TYPE_OBJECT",
     "GC_TYPE_OBJECT", "allocation-site copy of the baked header word"),
    ("crates/perry-codegen/src/lower_call/new_alloc.rs", "GC_FLAG_ARENA",
     "GC_FLAG_ARENA", "allocation-site copy of the baked header word"),
    ("crates/perry-codegen/src/lower_call/new_alloc.rs", "GC_LAYOUT_POINTER_FREE",
     "GC_LAYOUT_POINTER_FREE", "allocation-site copy of the baked header word"),
    ("crates/perry-codegen/src/lower_call/new_alloc.rs", "GC_OBJ_TYPED_LAYOUT_INTACT",
     "GC_OBJ_TYPED_LAYOUT_INTACT", "allocation-site copy of the baked header word"),

    # --- the class-field inline guard ---------------------------------------
    ("crates/perry-codegen/src/expr/class_field_inline_guard.rs", "GC_TYPE_OBJECT",
     "GC_TYPE_OBJECT", "guard: obj_type byte"),
    ("crates/perry-codegen/src/expr/class_field_inline_guard.rs", "GC_FLAG_FORWARDED_I8",
     "GC_FLAG_FORWARDED - 256", "guard: gc_flags 0x80 spelled as a signed i8"),
    ("crates/perry-codegen/src/expr/class_field_inline_guard.rs", "TYPED_LAYOUT_INTACT_BIT",
     "GC_OBJ_TYPED_LAYOUT_INTACT", "guard: raw-f64 slots need the intact bit"),
    ("crates/perry-codegen/src/expr/class_field_inline_guard.rs", "OBJ_FLAG_FROZEN_BIT",
     "OBJ_FLAG_FROZEN", "guard: a frozen receiver must route through the setter"),
    ("crates/perry-codegen/src/expr/class_field_inline_guard.rs",
     "OBJ_FLAG_PACKED_NUMERIC_PROOF_BIT", "OBJ_FLAG_PACKED_NUMERIC_PROOF",
     "guard: #8690 Array-subclass numeric-prefix proof"),
    ("crates/perry-codegen/src/expr/class_field_inline_guard.rs",
     "OBJ_FLAG_READ_FAST_PATH_BLOCKED",
     "OBJ_FLAG_ARRAY_DESCRIPTORS | OBJ_FLAG_HAS_DESCRIPTORS",
     "guard: #5654 per-receiver descriptor veto (a COMPOSITE of two flags)"),
    ("crates/perry-codegen/src/expr/class_field_inline_guard.rs",
     "OBJ_FLAG_WRITE_FAST_PATH_BLOCKED",
     "OBJ_FLAG_ARRAY_DESCRIPTORS | OBJ_FLAG_HAS_DESCRIPTORS"
     " | OBJ_FLAG_PACKED_NUMERIC_PROOF | OBJ_FLAG_FROZEN",
     "guard: the write side adds frozen + the packed-numeric proof"),

    # --- the element-shape guard's fused masks -------------------------------
    ("crates/perry-codegen/src/expr/element_shape_guard.rs", "GC_TYPE_ARRAY",
     "GC_TYPE_ARRAY", "element guard: obj_type byte"),
    ("crates/perry-codegen/src/expr/element_shape_guard.rs", "ELEM_HEADER_MASK",
     "0xFF | (GC_FLAG_FORWARDED << 8)"
     " | ((GC_OBJ_TYPED_LAYOUT_INTACT | OBJ_FLAG_HAS_DESCRIPTORS) << 16)",
     "element guard: one fused 32-bit mask over all three header bytes"),
    ("crates/perry-codegen/src/expr/element_shape_guard.rs", "ELEM_HEADER_EXPECT",
     "GC_TYPE_OBJECT | (GC_OBJ_TYPED_LAYOUT_INTACT << 16)",
     "element guard: the value ELEM_HEADER_MASK must produce"),
    ("crates/perry-codegen/src/expr/element_shape_guard.rs", "ELEM_HEADER_SHAPE_MASK",
     "0xFF | (GC_FLAG_FORWARDED << 8) | (OBJ_FLAG_HAS_DESCRIPTORS << 16)",
     "element guard: shape-keyed arm drops the intact conjunct"),
    ("crates/perry-codegen/src/expr/element_shape_guard.rs", "ELEM_HEADER_SHAPE_EXPECT",
     "GC_TYPE_OBJECT", "element guard: shape-keyed arm's expected value"),

    # --- the array-literal inline allocator's baked header word -------------
    ("crates/perry-codegen/src/expr/array_literal.rs", "GC_TYPE_ARRAY",
     "GC_TYPE_ARRAY", "array literal: obj_type byte of the baked header word"),
    ("crates/perry-codegen/src/expr/array_literal.rs", "GC_FLAG_ARENA",
     "GC_FLAG_ARENA", "array literal: gc_flags byte of the baked header word"),
    ("crates/perry-codegen/src/expr/array_literal.rs", "GC_LAYOUT_POINTER_FREE",
     "GC_LAYOUT_POINTER_FREE", "array literal: _reserved half of the baked header word"),

    # --- the typed-f64 receiver method probe's fused mask -------------------
    ("crates/perry-codegen/src/lower_call/method_override.rs",
     "GC_OBJECT_METHOD_GUARD_MASK_I32",
     "0xFF | (GC_FLAG_FORWARDED << 8)"
     " | ((OBJ_FLAG_HAS_DESCRIPTORS | OBJ_FLAG_PACKED_NUMERIC_PROOF) << 16)",
     "method probe: one fused 32-bit mask over all three header bytes"),
    ("crates/perry-codegen/src/lower_call/property_get/imported_object.rs",
     "GC_OBJECT_METHOD_GUARD_MASK_I32",
     "0xFF | (GC_FLAG_FORWARDED << 8)"
     " | ((OBJ_FLAG_HAS_DESCRIPTORS | OBJ_FLAG_PACKED_NUMERIC_PROOF) << 16)",
     "imported-object probe: second copy of the same fused mask"),

    # --- other single-bit restatements --------------------------------------
    ("crates/perry-codegen/src/expr/in_presence_ic.rs", "GC_TYPE_OBJECT",
     "GC_TYPE_OBJECT", "`in` presence IC: obj_type byte"),
    ("crates/perry-codegen/src/expr/in_presence_ic.rs", "GC_FLAG_FORWARDED",
     "GC_FLAG_FORWARDED", "`in` presence IC: not-forwarded"),
    ("crates/perry-codegen/src/expr/arrays_finds.rs", "GC_TYPE_MAP",
     "GC_TYPE_MAP", "Map find fast path: obj_type byte"),
    ("crates/perry-codegen/src/expr/arrays_finds.rs", "GC_FLAG_FORWARDED",
     "GC_FLAG_FORWARDED", "Map find fast path: not-forwarded"),
    ("crates/perry-codegen/src/lower_call/method_override.rs", "GC_TYPE_OBJECT",
     "GC_TYPE_OBJECT", "typed-f64 receiver method probe: obj_type byte"),
    ("crates/perry-codegen/src/lower_call/property_get/imported_object.rs", "GC_TYPE_OBJECT",
     "GC_TYPE_OBJECT", "imported-object property get: obj_type byte"),
    ("crates/perry-codegen/src/expr/proxy_reflect.rs", "PLAIN_ORDINARY_OBJ_FLAG",
     "OBJ_FLAG_PLAIN_ORDINARY", "proxy/reflect: plain-ordinary veto"),
    ("crates/perry-codegen/src/expr/proxy_reflect_write_ic.rs", "STABLE_TOMBSTONES_OBJ_FLAG",
     "OBJ_FLAG_STABLE_TOMBSTONES", "proxy write IC: stable-tombstones veto"),
    ("crates/perry-codegen/src/codegen/string_pool.rs", "GC_LAYOUT_AND_INTACT_MASK",
     "GC_LAYOUT_POINTER_FREE | GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT",
     "module init: the header-image layout bits it may rewrite"),
    ("crates/perry-codegen/src/codegen/string_pool.rs", "GC_SIDE_MASK_AND_INTACT",
     "GC_LAYOUT_SIDE_MASK | GC_OBJ_TYPED_LAYOUT_INTACT",
     "module init: the side-mask + intact pair it writes"),
]

# Declared constants this checker deliberately does not anchor, each with the
# reason. Enumerated rather than ignored: a reader should be able to see the
# whole duplicated surface in one place, including the parts out of scope.
OUT_OF_SCOPE = {
    ("crates/perry-codegen/src/target_layout.rs", "GC_HEADER_SIZE_BYTES"):
        "a STRUCT SIZE, not a bit assignment; the runtime asserts it structurally",
    ("crates/perry-codegen/src/lower_call/new_alloc.rs", "GC_HEADER_SIZE"):
        "same struct size, re-derived at the allocation site",
    ("crates/perry-codegen/src/expr/array_literal.rs", "GC_HEADER_SIZE"):
        "same struct size, re-derived at the array-literal site",
    ("crates/perry-codegen/src/gc_map.rs", "GC_MAP_MAGIC"):
        "`.perry_gcmap` section format, not the object header",
    ("crates/perry-codegen/src/gc_map.rs", "GC_MAP_VERSION"):
        "`.perry_gcmap` section format, not the object header",
    ("crates/perry-codegen/src/gc_map.rs", "GC_MAP_LABEL"):
        "`.perry_gcmap` section format, not the object header",
}

# Prefixes that make a codegen `const` look like a header restatement. A new
# declaration matching one of these must be registered above or exempted in
# OUT_OF_SCOPE, so the next one cannot arrive silently — the rule
# `check_node_version_consistency.py` uses for `node-version:` literals.
WATCHED = re.compile(r"^(GC_|OBJ_|TYPED_LAYOUT|PLAIN_ORDINARY_OBJ|STABLE_TOMBSTONES_OBJ|ELEM_HEADER)")

CONST_RE = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?const\s+([A-Za-z_][A-Za-z0-9_]*)\s*:\s*[^=]+=\s*([^;]+);"
)


def parse_consts(path: Path) -> dict[str, str]:
    out: dict[str, str] = {}
    for line in path.read_text().splitlines():
        m = CONST_RE.match(line)
        if m:
            out.setdefault(m.group(1), m.group(2).strip())
    return out


def literal_value(raw: str) -> int | None:
    """The integer a declaration's right-hand side denotes, or None."""
    text = raw.strip().strip('"').strip()
    text = re.sub(r"_", "", text)
    try:
        return int(text, 0)
    except ValueError:
        return None


def runtime_values(root: Path) -> tuple[dict[str, int], list[str]]:
    values: dict[str, int] = {}
    problems: list[str] = []
    for rel in RUNTIME_SOURCES:
        path = root / rel
        if not path.exists():
            problems.append(f"runtime source missing: {rel}")
            continue
        for name, raw in parse_consts(path).items():
            if name in RUNTIME_WANTED and name not in values:
                v = literal_value(raw)
                if v is not None:
                    values[name] = v
    for name in sorted(RUNTIME_WANTED - values.keys()):
        problems.append(
            f"runtime constant {name} not found in {' or '.join(RUNTIME_SOURCES)} — "
            "it was renamed or moved; update RUNTIME_SOURCES/RUNTIME_WANTED so the "
            "restatements that derive from it stay anchored"
        )
    return values, problems


def check(root: Path) -> list[str]:
    values, problems = runtime_values(root)
    if problems:
        return problems

    declared: dict[tuple[str, str], str] = {}
    for rel in sorted({entry[0] for entry in REGISTRY} | {k[0] for k in OUT_OF_SCOPE}):
        path = root / rel
        if not path.exists():
            problems.append(f"registered file missing: {rel}")
            continue
        for name, raw in parse_consts(path).items():
            declared[(rel, name)] = raw

    for rel, const, expr, why in REGISTRY:
        raw = declared.get((rel, const))
        if raw is None:
            problems.append(
                f"{rel}: registered constant {const} no longer declared "
                f"({why}) — delete its REGISTRY entry in the same commit"
            )
            continue
        got = literal_value(raw)
        if got is None:
            problems.append(f"{rel}:{const} = {raw!r} is not an integer literal")
            continue
        want = eval(expr, {"__builtins__": {}}, dict(values))  # noqa: S307 - fixed table
        if got != want:
            problems.append(
                f"{rel}:{const} = {got} (0x{got:X}) but the runtime says "
                f"{want} (0x{want:X})\n"
                f"    derivation: {expr}\n"
                f"    role:       {why}\n"
                f"    The compiler bakes this into emitted code and does NOT link "
                f"perry-runtime, so a mismatch ships a binary whose objects the "
                f"collector misreads. Fix the compiler side, or update the "
                f"derivation if the runtime deliberately moved the bit."
            )

    # Nothing header-shaped may arrive unregistered.
    watched_files = {entry[0] for entry in REGISTRY} | {k[0] for k in OUT_OF_SCOPE}
    known = {(rel, const) for rel, const, _, _ in REGISTRY} | set(OUT_OF_SCOPE)
    for (rel, const) in sorted(declared):
        if rel in watched_files and WATCHED.match(const) and (rel, const) not in known:
            problems.append(
                f"{rel}: {const} looks like a GC header restatement but is not "
                "registered. Add it to REGISTRY with the expression that "
                "re-derives it from perry-runtime, or to OUT_OF_SCOPE with a reason."
            )
    return problems


def self_test(root: Path) -> int:
    """Prove the checker can fail: perturb one runtime value and expect a report."""
    values, _ = runtime_values(root)
    saved = values["GC_OBJ_TYPED_LAYOUT_INTACT"]
    rel, const, expr, why = next(
        e for e in REGISTRY if e[1] == "TYPED_LAYOUT_INTACT_BIT"
    )
    raw = parse_consts(root / rel).get(const)
    got = literal_value(raw)
    perturbed = dict(values)
    perturbed["GC_OBJ_TYPED_LAYOUT_INTACT"] = saved << 1
    want = eval(expr, {"__builtins__": {}}, perturbed)  # noqa: S307
    if got == want:
        print("self-test FAILED: a moved intact bit was not detected", file=sys.stderr)
        return 1
    if check(root):
        print("self-test FAILED: the tree is already red", file=sys.stderr)
        return 1
    print(
        "check_gc_header_constants self-test: OK — a one-bit move of "
        "GC_OBJ_TYPED_LAYOUT_INTACT is detected, and the tree is currently clean"
    )
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--list", action="store_true", help="describe what is pinned")
    ap.add_argument("--self-test", action="store_true", help="prove the checker can fail")
    args = ap.parse_args()

    if args.self_test:
        return self_test(REPO)

    if args.list:
        values, _ = runtime_values(REPO)
        print("Authoritative runtime values:")
        for name in sorted(values):
            print(f"  {name:32s} = {values[name]} (0x{values[name]:X})")
        print(f"\nCompiler-side restatements pinned ({len(REGISTRY)}):")
        for rel, const, expr, why in REGISTRY:
            print(f"  {rel}\n      {const} = {expr}\n      {why}")
        print(f"\nDeclared but out of scope ({len(OUT_OF_SCOPE)}):")
        for (rel, const), reason in sorted(OUT_OF_SCOPE.items()):
            print(f"  {rel}:{const} — {reason}")
        return 0

    problems = check(REPO)
    if problems:
        print("check_gc_header_constants: FAILED\n", file=sys.stderr)
        for p in problems:
            print(f"  - {p}\n", file=sys.stderr)
        return 1
    print(
        f"check_gc_header_constants: OK — {len(REGISTRY)} compiler-side header "
        f"restatements agree with perry-runtime "
        f"({len(OUT_OF_SCOPE)} declared constants out of scope, listed with reasons)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
