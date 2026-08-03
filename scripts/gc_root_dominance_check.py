#!/usr/bin/env python3
"""Static GC root-dominance checker for perry-emitted LLVM IR.

Invariant checked
-----------------
For any GC-managed value materialized in a function, the shadow-slot root
store that makes it visible to the precise-root collector must DOMINATE (in
the CFG sense: be on every path before) any subsequent site that can trigger
a collection.

A violation is a triple (alloc A, activating bind B, collecting call C) where
there exists a CFG path  A -> ... -> C -> ... -> B, i.e. the value is live in
an SSA register / an unrooted alloca while a collection can run.

Why this exists (#7154)
-----------------------
Two bugs of this exact class have shipped. #7184: the root store was emitted
but its slot index fell outside the pushed shadow frame, so
`js_shadow_slot_bind` bounds-checked it and silently no-opped. #7186: the root
store was emitted in-frame but *after* a call that allocates, so a back-edge
poll inside that call ran an evacuating minor while the value was live only in
an SSA register. Both present identically — a rooted slot holding a dangling
pointer, surfacing cycles later as "TypeError: value is not a function" — and
neither is visible to any runtime GC probe, because at the moment of the
collection there is nothing for the collector to find.

This checker is the instrument that finds them statically, over a whole corpus,
before they reach a crash.

Usage
-----
    # dump the IR (writes .perry-trace/llvm/*.ll)
    PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_INLINE_SHADOW_SLOT=0 \
        perry compile app.ts -o /tmp/app --trace llvm

    python3 scripts/gc_root_dominance_check.py .perry-trace/llvm [-v]
    python3 scripts/gc_root_dominance_check.py .perry-trace/llvm --any-def
    python3 scripts/gc_root_dominance_check.py .perry-trace/llvm --moving-only

`PERRY_INLINE_SHADOW_SLOT=0` makes every root store the `@js_shadow_slot_bind`
call form; the inline #7088 diamond is equivalent but harder to anchor on.
`PERRY_GC_MOVING_LOOP_POLLS=1` is what puts `js_gc_loop_safepoint` in the IR,
which is what the `MOVING` classification keys on.

Modes
-----
`--any-def` (default off) anchors on ANY call whose result reaches the root
store, not just the known allocation intrinsics — broader, noisier.
`--moving-only` keeps only violations whose window contains a call that can
transitively reach a moving-minor safepoint.

Soundness
---------
One-sided by design. `NONCOLLECTING` is the only place a call is declared safe,
and every entry names the runtime source line that proves it; anything
unrecognised counts as a collection point. A missing entry costs a false
positive, never a missed bug. Dominance is real CFG dominance (Cooper/Harvey/
Kennedy) and the window is path-based, so a loop back edge is not mistaken for
an intra-iteration path — a naive line-order scan produced 8 false positives
where this reports none.

Exit code is 1 when any violation is reported, so it can gate.

Gating
------
A dominance gate that cannot fail is worse than none, so three things are
enforced rather than assumed (CLAUDE.md, "four ways a gate can be unable to
fail"):

* **empty input is an error, not a pass.** No paths, a path that holds no
  `.ll`, or a typo'd flag exits 2 with a message instead of printing
  `violations: 0`.
* **the subject must be live.** `--min-files` / `--min-binds` assert that the
  corpus actually contained modules and actually contained root stores. A green
  verdict over zero binds proves nothing and is refused.
* **malformed IR is an error, not a silent skip.** A function body whose first
  line is not a label used to parse to zero blocks and be dropped without a
  word — a planted violation vanished and the run exited 0. Both that and a
  label-shaped line the parser cannot read now raise.

`--self-test` compiles two in-file fixtures — one with a planted violation of
exactly this class, one identical but with the root store hoisted above the
collection point — and asserts the checker reports the first and clears the
second. It is the "assert the gate can fail" arm, and CI runs it next to the
real corpus.
"""
import argparse
import contextlib
import io
import json
import os
import re
import sys
import tempfile
from collections import defaultdict, deque

# ---------------------------------------------------------------- IR parsing

DEFINE_RE = re.compile(r"^define\s+.*?@([\w.$]+)\(")
# A trailing `; preds = %a, %b` comment is LLVM's own printed form (`llvm-dis`,
# `opt -S`, `clang -S -emit-llvm`). Perry's writer never emits it, but pointing
# this tool at LLVM-canonical IR is the obvious thing to try, and rejecting the
# label there used to append it to the PREVIOUS block — collapsing the whole
# function into one and producing exactly the line-order false positives the
# docstring above says real dominance avoids.
LABEL_RE = re.compile(r"^([\w.$][\w.$]*):\s*(?:;.*)?$")
# Anything that ends in `:` and is not an instruction is label-SHAPED. If the
# strict form above declines it, that is a parser gap and must be loud.
LABEL_SHAPED_RE = re.compile(r"^[^\s=]+:\s*(?:;.*)?$")
ASSIGN_RE = re.compile(r"^\s*%([\w.$]+)\s*=\s*(.*)$")
# `invoke` (#7302) is a call with an unwind edge — it must be seen as a call
# here or every collecting call inside a `try` body would be invisible to the
# dominance analysis (a silent false-green for exactly the functions where
# rooting is hardest).
CALL_RE = re.compile(r"\b(?:call|invoke)\s+[^@]*@([\w.$]+)\(")
BIND_RE = re.compile(r"call void @js_shadow_slot_bind\(i32 (\d+), ptr %([\w.$]+)\)")
CLEAR_RE = re.compile(r"call void @js_shadow_slot_set\(i32 (\d+), i64 0\)")
STORE_RE = re.compile(r"^\s*store\s+([\w\[\]x* ]+?)\s+([^,]+),\s*ptr %([\w.$]+)")
BR_UNCOND_RE = re.compile(r"^\s*br label %([\w.$]+)")
BR_COND_RE = re.compile(r"^\s*br i1 [^,]+, label %([\w.$]+), label %([\w.$]+)")
SWITCH_LABEL_RE = re.compile(r"label %([\w.$]+)")
# Invoke edges (#7302): normal destination + unwind destination. The invoke
# terminates its block; the continuation label follows immediately in the
# emitted text and both successors must appear in the CFG or the landing pad
# (and everything reached through it) would be dropped as unreachable.
INVOKE_EDGE_RE = re.compile(r"\binvoke\b.*\bto label %([\w.$]+) unwind label %([\w.$]+)")


class Insn:
    __slots__ = ("text", "block", "idx", "result", "callee")

    def __init__(self, text, block, idx):
        self.text = text
        self.block = block
        self.idx = idx
        m = ASSIGN_RE.match(text)
        self.result = m.group(1) if m else None
        c = CALL_RE.search(text)
        self.callee = c.group(1) if c else None


class Func:
    def __init__(self, name):
        self.name = name
        self.blocks = []               # ordered block labels
        self.insns = defaultdict(list)  # label -> [Insn]
        self.succs = defaultdict(set)
        self.preds = defaultdict(set)


class MalformedIR(Exception):
    """The parser cannot model this IR, so any verdict over it would be a lie.

    Raised rather than skipped: a function the parser drops reports zero
    violations, which is indistinguishable from a clean one.
    """


def parse_file(path):
    funcs = []
    cur = None
    curblk = None
    with open(path, "r", errors="replace") as fh:
        for lineno, raw in enumerate(fh, 1):
            line = raw.rstrip("\n")
            m = DEFINE_RE.match(line)
            if m:
                cur = Func(m.group(1))
                funcs.append(cur)
                curblk = None
                continue
            if cur is None:
                continue
            if line.startswith("}"):
                if not cur.blocks:
                    raise MalformedIR(
                        f"{path}:{lineno}: @{cur.name} has no basic-block label. "
                        "The parser has no way to build a CFG for it, and a "
                        "silent skip would report it clean."
                    )
                cur = None
                curblk = None
                continue
            lm = LABEL_RE.match(line)
            if lm:
                curblk = lm.group(1)
                if curblk not in cur.insns:
                    cur.blocks.append(curblk)
                    cur.insns[curblk] = []
                continue
            if not line.strip():
                continue
            if LABEL_SHAPED_RE.match(line.strip()):
                raise MalformedIR(
                    f"{path}:{lineno}: label-shaped line the parser cannot read: "
                    f"{line.strip()!r}. Appending it to the previous block would "
                    "merge two blocks and fabricate an intra-block path."
                )
            if curblk is None:
                # An instruction before the first label is LLVM's implicit
                # entry block (`%0`). Perry's writer never emits one, but if it
                # ever does, or if this is run over `llvm-dis` output, dropping
                # the instructions would hide every violation in the entry
                # block — which is where allocations live.
                raise MalformedIR(
                    f"{path}:{lineno}: instruction before the first label in "
                    f"@{cur.name}: {line.strip()!r} (implicit entry block)."
                )
            cur.insns[curblk].append(Insn(line, curblk, len(cur.insns[curblk])))
    for f in funcs:
        build_cfg(f)
    return funcs


def build_cfg(f):
    for b in f.blocks:
        for ins in f.insns[b]:
            t = ins.text
            m = BR_COND_RE.match(t)
            if m:
                f.succs[b].add(m.group(1))
                f.succs[b].add(m.group(2))
                continue
            m = BR_UNCOND_RE.match(t)
            if m:
                f.succs[b].add(m.group(1))
                continue
            if t.strip().startswith("switch"):
                for lbl in SWITCH_LABEL_RE.findall(t):
                    f.succs[b].add(lbl)
                continue
            m = INVOKE_EDGE_RE.search(t)
            if m:
                f.succs[b].add(m.group(1))
                f.succs[b].add(m.group(2))
    for b, ss in list(f.succs.items()):
        for s in ss:
            f.preds[s].add(b)


# ------------------------------------------------------- collection-site model

# Runtime helpers that provably cannot allocate, run user code, or poll.
NONCOLLECTING = {
    # shadow stack / roots
    "js_shadow_slot_bind", "js_shadow_slot_set", "js_shadow_frame_enter",
    "js_shadow_frame_push", "js_shadow_frame_pop", "js_shadow_state_addr",
    "js_gc_temp_root_push", "js_gc_temp_root_get", "js_gc_temp_root_set",
    "js_gc_temp_root_truncate",
    # layout / barrier bookkeeping (no allocation)
    "js_gc_init_typed_shape_layout", "js_gc_layout_note_slot",
    "js_write_barrier_root_nanbox", "js_write_barrier_slot",
    "js_runtime_write_barrier_slot", "js_gc_register_global_root",
    # pure value predicates / bit twiddling
    "js_is_truthy", "js_nanbox_get_pointer", "js_value_is_object",
    "js_value_is_string", "js_typeof_tag",
    # inline-cache guards: pure reads
    "js_typed_feedback_closure_direct_call_guard",
    "js_typed_feedback_shape_guard", "js_typed_feedback_note",
    # ctor identity selection
    "js_ctor_return_override",
    "llvm.lifetime.start.p0", "llvm.lifetime.end.p0",
    # verified non-allocating bookkeeping stores/reads (perry-runtime)
    "js_closure_set_capture_bits",   # closure/alloc.rs:477 raw slot write + layout note
    "js_closure_get_capture_bits",   # closure/alloc.rs:463 raw slot read
    "js_closure_set_capture_ptr", "js_closure_get_capture_ptr",
    "js_box_set_bits", "js_box_get_bits",           # box.rs:317 raw cell write
    "js_i32_box_set", "js_bool_box_set",
    "js_write_barrier",                              # gc/barrier.rs:930
    "js_tdz_suppress_begin", "js_tdz_suppress_end",  # box.rs:242/248 counter
    "js_array_note_numeric_write",                   # array/header.rs:1443
    "js_array_length",                               # array/indexing.rs:537
    "js_object_mark_class", "js_class_object_pin_parent",
    "js_new_target_get", "js_new_target_set",
    # closure/unbox.rs:25 -- a tag check on the NaN-boxed callee and a low-48
    # mask. No allocation, no user code, no poll. It sits between every
    # dynamic call's last argument and its `js_closure_callN`, so leaving it
    # out reported the whole argument list of every 1-arg dynamic call as
    # stale (372 of the 729 fatal sinks) with `MOVING: no`.
    #
    # `js_closure_unbox_callee_checked_rebind` is deliberately NOT here: it
    # calls `clone_closure_rebind_this`, which allocates a replacement closure
    # (closure/dynamic_props.rs:1040) when the callee captures `this`. That one
    # IS a collection point, and #7154's fix re-reads the arguments below it.
    "js_closure_unbox_callee_checked",
    # object/this_binding.rs:160 -- a thread-local cell swap
    "js_implicit_this_set", "js_implicit_this_get",
    "js_gc_note_slot_layout", "js_string_addref_if_heap_string",
    # `js_get_string_pointer_unified` is deliberately NOT here. Its SSO branch
    # calls `js_string_materialize_to_heap`, which allocates (value/nanbox.rs:268),
    # so it is a collection point by this file's one-sided rule even though the
    # collection it can cause is currently always non-moving. See #7213.
}

# The single site where an evacuating (moving) minor runs.
MOVING_POLL = "js_gc_loop_safepoint"

# Result-producing calls that materialize a fresh GC object.
#
# ------------------------------------------------------------------ THE AUDIT
# This list is enumerated from the runtime's actual exported entry points, NOT
# guessed from a naming convention, because guessing a convention is precisely
# how it has failed twice:
#
#   * `js_implicit_this_set` (#7226) -- a root READ that was not modelled, so
#     `prev_this` survived two PRs that were looking straight at it;
#   * `js_regexp_new` (#7154, this change) -- the regex carried an alternative
#     spelled `regexp_alloc\w*`, and **no such symbol has ever existed**. The
#     `/re/.test(s)` lowering holds `js_regexp_new`'s raw result in a register
#     across `js_jsvalue_to_string_coerce`, and the checker reported nothing
#     because the register had no recognised heap-value source.
#
# The second one is the instructive one. `regexp_alloc\w*` was not a typo, it
# was an ASSUMED convention: whoever wrote it knew a RegExp allocates and
# extrapolated the `_alloc` suffix from its neighbours. Reconciling every
# alternative below against the real symbol table (`grep -rhoE 'extern "C" fn
# js_\w+'` over perry-runtime + perry-stdlib, intersected with the names
# perry-codegen actually declares) found FOUR alternatives in the same state --
# `regexp_alloc\w*`, `promise_alloc\w*`, `bigint_alloc\w*` and
# `typed_array_alloc\w*` matched nothing whatsoever. A quarter of the pattern
# was decorative.
#
# The root cause is that the runtime materializes fresh GC objects under THREE
# naming conventions and the old pattern modelled one:
#
#   `_alloc*`   js_object_alloc, js_array_alloc, js_closure_alloc, js_box_alloc,
#               js_map_alloc, js_set_alloc, js_buffer_alloc, js_uint8array_alloc,
#               js_arguments_object_alloc, js_inline_arena_slow_alloc, ...
#   `_new*`     js_regexp_new, js_promise_new, js_symbol_new, js_date_new,
#               js_error_new, js_typed_array_new, js_weakmap_new, js_weakset_new,
#               js_url_new, js_boxed_string_new, js_array_buffer_new, ~140 more
#   `_create*`  js_object_create, js_array_create, js_vm_create_context,
#               js_crypto_create_hash, js_readline_create_interface, ~40 more
#
# So the three conventions are matched as conventions, and the constructors
# that use none of them are enumerated explicitly below. Widening is SAFE in
# the checker's one-sided direction: a name that is in fact not an allocation
# costs a false positive to triage, while a missing one costs a shipped
# use-after-free plus the investigation round it takes to find it by hand.
# When in doubt, add it.
ALLOC_RE = re.compile(
    r"^js_("
    # -- convention 1: `*_alloc*`, including the arena's own slow path.
    r"\w*_alloc\w*|"
    # -- convention 2: `*_new*`. `js_regexp_new` is the #7154 residual.
    r"\w*_new\w*|"
    # -- convention 3: `*_create*`.
    r"\w*_create\w*|create_\w+|"
    # -- constructors using none of the three conventions -------------------
    # `new X(...)` / `X(...)` forms folded at HIR into a direct ctor call.
    r"\w*_construct|\w*_construct_call|\w*_construct_apply|"
    r"reflect_construct|new_function_construct\w*|super_construct_apply|"
    # fresh strings. Every one of these returns a *mut StringHeader the
    # caller holds raw; `string_coerce` and `jsvalue_to_string_coerce` are
    # the two the `/re/` lowerings feed.
    r"string_concat\w*|string_coerce|string_from\w*|string_append|"
    r"jsvalue_to_string\w*|value_to_string\w*|number_to_string\w*|"
    r"string_repeat|string_slice|string_substring|string_substr|"
    r"string_pad_\w+|string_trim\w*|string_to_\w+_case|string_replace\w*|"
    r"string_split|string_normalize|string_at|string_char_at|"
    r"string_index_get_boxed|boxed_string\w*|"
    # fresh arrays: the copy-on-read Array.prototype methods, the ES2023
    # change-by-copy family, and the iterable/array-like converters.
    r"array_from\w*|array_clone\w*|array_concat\w*|array_slice|array_splice|"
    r"array_to_spliced|array_to_sorted\w*|array_to_reversed|array_with|"
    r"array_flat\w*|array_map|array_filter|array_like_to_array|"
    r"iterator_to_array|"
    # fresh objects/collections handed back as a whole
    r"object_keys\w*|object_values\w*|object_entries\w*|object_from_entries|"
    r"object_assign\w*|object_group_by|object_coerce|"
    r"object_get_own_property_descriptor\w*|object_get_own_property_names|"
    r"object_get_own_property_symbols|"
    r"map_from_iterable|set_from_iterable|map_group_by|"
    r"set_union|set_intersection|set_difference|set_symmetric_difference|"
    r"structured_clone\w*|"
    # module namespace objects, class-shape side tables, generator plumbing
    r"build_class_keys_array|create_namespace|create_native_module_namespace|"
    r"generator_attach_prototype|proxy_revocable|"
    # BigInt: no `_alloc` and no `_new`; the constructors are `_from_*`.
    #
    # NOT COVERED, AND DELIBERATELY LEFT UNCOVERED HERE: BigInt *arithmetic*
    # (`js_bigint_add`, `js_bigint_and`, `js_bigint_mul`, ...) allocates a fresh
    # BigInt and matches none of the three conventions. An alternative spelled
    # `bigint_\w+_op` used to sit here and matched nothing -- the same
    # extrapolated-suffix mistake as `regexp_alloc\w*`, in the same list, added
    # by the change that removed the first four. It is deleted rather than
    # widened because widening it is a coverage decision with its own false
    # positives (`js_bigint_equals` returns a bool), and that belongs in a
    # change that can measure the new hits. Tracked in the PR thread.
    r"bigint_from\w*|"
    # Buffers / typed arrays that spell it neither way. `array_buffer_slice`
    # and `typed_array_from\w*` were removed for the same reason: no such
    # symbols. The real one, `js_buffer_from_arraybuffer_slice`, is already
    # matched by `buffer_from\w*`.
    r"buffer_from\w*"
    r")$"
)

# --------------------------------------------------------------------- AUDIT
#
# A regex alternative that matches nothing is the gate-can't-fail pattern in
# regex form: it reads as coverage, it costs nothing to keep, and it is
# indistinguishable from real coverage until a bug slips through the hole it
# was supposed to close. This has now happened twice in this one pattern:
#
#   * `regexp_alloc\w*`, `promise_alloc\w*`, `bigint_alloc\w*`,
#     `typed_array_alloc\w*` -- four dead alternatives, found only because
#     `js_regexp_new` cost #7154 a full investigation round;
#   * `array_of`, `array_group_by`, `bigint_\w+_op`, `typed_array_from\w*`,
#     `array_buffer_slice` -- five MORE, introduced by the change that removed
#     the first four, and found by running this auditor against it.
#
# The second round is the argument for automating it. A prose audit does not
# survive its own next edit; the enumeration below is checked by CI instead
# (`--audit-alloc-re`), so an alternative cannot go dead without going red.
#
# Deleting a dead alternative changes NOTHING about what the checker matches --
# that is what "matches no symbol" means -- so this is not a narrowing.

_EXTERN_C_FN_RE = re.compile(r'extern\s+"C"\s+fn\s+(js_\w+)')

# The runtime crates that export the C-ABI surface perry-codegen calls.
SYMBOL_ROOTS = ("crates/perry-runtime/src", "crates/perry-stdlib/src")


def runtime_symbols(roots=SYMBOL_ROOTS):
    """Every `extern "C" fn js_*` the runtime actually exports."""
    syms = set()
    for root in roots:
        if not os.path.isdir(root):
            continue
        for dirpath, _dirs, files in os.walk(root):
            for name in files:
                if not name.endswith(".rs"):
                    continue
                with open(os.path.join(dirpath, name),
                          encoding="utf-8", errors="replace") as fh:
                    syms.update(_EXTERN_C_FN_RE.findall(fh.read()))
    return syms


def alloc_re_alternatives():
    """The top-level alternatives inside ALLOC_RE's `js_(...)` group.

    Asserts the pattern's shape rather than assuming it: if ALLOC_RE is ever
    rewritten into a form this cannot decompose, that must be a loud failure,
    not an audit that silently checks zero alternatives.
    """
    pat = ALLOC_RE.pattern
    if not (pat.startswith("^js_(") and pat.endswith(")$")):
        raise MalformedIR(
            "ALLOC_RE is no longer of the form ^js_(...)$; the alternative "
            "audit cannot decompose it. Update alloc_re_alternatives() rather "
            "than leaving the audit silently vacuous.")
    inner = pat[len("^js_("):-len(")$")]
    if "(" in inner:
        raise MalformedIR(
            "ALLOC_RE now contains a nested group; splitting on '|' would "
            "produce bogus alternatives. Update alloc_re_alternatives().")
    return [a for a in inner.split("|") if a]


def dead_alloc_alternatives(symbols):
    """ALLOC_RE alternatives matching none of `symbols`, in pattern order."""
    dead = []
    for alt in alloc_re_alternatives():
        probe = re.compile("^js_(?:%s)$" % alt)
        if not any(probe.match(s) for s in symbols):
            dead.append(alt)
    return dead


def audit_alloc_re(roots=SYMBOL_ROOTS):
    """Exit status for `--audit-alloc-re`. 0 clean, 2 on a dead alternative."""
    syms = runtime_symbols(roots)
    # Non-vacuity first: an empty symbol set would report every alternative
    # dead, and a tiny one would report a plausible-looking few. Either way the
    # verdict would be about the scan, not about the pattern.
    if len(syms) < 500:
        print(f"error: found only {len(syms)} `extern \"C\" fn js_*` symbols "
              f"under {', '.join(roots)}. The audit is measuring its own scan, "
              "not ALLOC_RE. Run it from the repository root.", file=sys.stderr)
        return 2
    dead = dead_alloc_alternatives(syms)
    print(f"=== ALLOC_RE: {len(alloc_re_alternatives())} alternatives vs "
          f"{len(syms)} exported js_* symbols")
    if dead:
        print("error: ALLOC_RE alternatives that match no runtime symbol:",
              file=sys.stderr)
        for a in dead:
            print(f"  {a}", file=sys.stderr)
        print("An alternative that matches nothing reads as coverage and is "
              "not. Either it is misspelled -- check the real name in the "
              "runtime -- or the symbol was renamed or removed and the "
              "alternative should go with it.", file=sys.stderr)
        return 2
    print("=== every alternative matches at least one exported symbol")
    return 0


def dead_poll_capable(symbols):
    """`POLL_CAPABLE_RUNTIME` entries that name no exported symbol, sorted.

    No exceptions, deliberately — `MOVING_POLL` itself is a real export
    (`gc/policy.rs:1766`), so carving one out would be an unearned hole in the
    very audit that exists because unearned holes are the recurring bug here.
    """
    return sorted(n for n in POLL_CAPABLE_RUNTIME if n not in symbols)


def audit_poll_capable(roots=SYMBOL_ROOTS):
    """Exit status for `--audit-poll-capable`. 0 clean, 2 on a phantom entry.

    Same hazard as `--audit-alloc-re`, in the other direction. `ALLOC_RE`
    decides whether a register HAS a heap-value source; `POLL_CAPABLE_RUNTIME`
    decides whether the window around it is MOVING, which is what
    `gc-root-dominance.yml`'s `--moving-only` arms gate on. An entry that names
    nothing reads as coverage and suppresses nothing, and unlike a dead
    ALLOC_RE alternative it is invisible even to a careful reader, because the
    misspelling is usually a *plausible* name for a real operation.

    Measured when this auditor was written: TEN of twenty-eight entries were
    phantoms, including four separate spellings of "call a JS closure", none of
    which existed. See the block comment on `POLL_CAPABLE_RUNTIME`.
    """
    syms = runtime_symbols(roots)
    # Non-vacuity first, for the same reason `audit_alloc_re` does it: an empty
    # or tiny symbol table would report every entry dead and the verdict would
    # be about the scan.
    if len(syms) < 500:
        print(f"error: found only {len(syms)} `extern \"C\" fn js_*` symbols "
              f"under {', '.join(roots)}. The audit is measuring its own scan, "
              "not POLL_CAPABLE_RUNTIME. Run it from the repository root.",
              file=sys.stderr)
        return 2
    dead = dead_poll_capable(syms)
    print(f"=== POLL_CAPABLE_RUNTIME: {len(POLL_CAPABLE_RUNTIME)} entries vs "
          f"{len(syms)} exported js_* symbols")
    if dead:
        print("error: POLL_CAPABLE_RUNTIME entries that name no runtime symbol:",
              file=sys.stderr)
        for n in dead:
            print(f"  {n}", file=sys.stderr)
        print("An entry that matches nothing cannot classify any window as "
              "MOVING, so it reads as coverage of an operation the "
              "`--moving-only` gate is in fact blind to. Replace it with the "
              "symbol codegen actually emits -- do not just delete it, or the "
              "audit goes green and the hole stays.", file=sys.stderr)
        return 2
    print("=== every entry names an exported runtime symbol")
    return 0

# Bit-level / identity producers a heap address flows through unchanged.
TRANSPARENT_OPS = ("or i64", "and i64", "bitcast", "inttoptr", "ptrtoint",
                   "select", "phi", "add i64", "sub i64")
TRANSPARENT_CALLS = {"js_ctor_return_override"}
# Calls that ROOT their argument (protecting it from that point on).
# `js_box_set_bits` publishes into a mutable-capture box, which `BOX_REGISTRY`
# / `scan_box_roots_mut` marks AND rewrites (gc/mod.rs:547).
ROOTING_CALLS = {"js_gc_temp_root_push", "js_gc_temp_root_set",
                 "js_box_set_bits", "js_i32_box_set", "js_bool_box_set"}


def operand_regs(text):
    """SSA registers referenced on the right-hand side of an instruction."""
    body = text.split(" = ", 1)[-1] if " = " in text else text
    return set(re.findall(r"%([\w.$]+)", body))


def is_transparent(ins):
    if ins.callee in TRANSPARENT_CALLS:
        return True
    if ins.callee is not None:
        return False
    return any(op in ins.text for op in TRANSPARENT_OPS)


def provenance(def_of, reg, limit=64):
    """Walk back from `reg` through bit-level/identity producers to the
    instructions that actually MATERIALIZE the value (calls and loads)."""
    origins = []
    seen = set()
    q = deque([reg])
    while q and len(seen) < limit:
        r = q.popleft()
        if r in seen:
            continue
        seen.add(r)
        d = def_of.get(r)
        if d is None:
            continue
        if is_transparent(d):
            q.extend(operand_regs(d.text))
            continue
        if d.callee is not None or " load " in d.text or d.text.strip().startswith("%") and "= load" in d.text:
            origins.append(d)
    return origins


def uses(text, regs):
    """Does `text` reference any of `regs` as a whole SSA operand?
    (`%r1` must NOT match `%r16` -- a substring test silently taints the
    entire rest of the function.)"""
    for r in regs:
        if re.search(r"%" + re.escape(r) + r"(?![\w.$])", text):
            return True
    return False


def is_collecting(callee):
    if callee is None:
        return False
    if callee in NONCOLLECTING:
        return False
    if callee.startswith("llvm."):
        return False
    return True


# ------------------------------------------------- interprocedural poll reach

# Runtime helpers that re-enter compiled JS (and therefore its back-edge polls).
#
# The COERCION family is the half of this set that is easy to leave out, and
# leaving it out is what kept `--moving-only` blind to #7154's `/re/.test(s)`
# residual even once `ALLOC_RE` had been widened to recognise `js_regexp_new`.
# A coercion does not *look* like a call into user code, but ToPrimitive is
# exactly that: `js_jsvalue_to_string_coerce` runs `to_string_method_impl(…,
# skip_to_primitive = false)`, which consults `[Symbol.toPrimitive]`, then
# `toString`, then `valueOf` — arbitrary JS, with its own loop back-edge polls.
# `js_string_coerce`'s own doc comment already said so ("a `POINTER_TAG` object
# routes through `js_jsvalue_to_string`, which can invoke a user `toString` /
# `valueOf`"); the checker just never read it.
#
# This matters more than the raw-count modes suggest, because `--moving-only`
# is the mode the `gc-root-dominance.yml` gate runs. A source the gate cannot
# classify as reaching a moving minor is a source the gate cannot fail on.
#
# ## The property-GET hole (#7154 follow-up)
#
# Membership here is by EXACT emitted symbol, and that is the trap. The set
# carried `js_object_get_field_by_name` and `js_object_set_field_by_name` as a
# matched pair, which reads like symmetric coverage of property access. It is
# not. Codegen emits `js_object_set_field_by_name` verbatim (27 call sites in
# perry-codegen, 205 in the gate's own corpus) but never emits
# `js_object_get_field_by_name` at all — the GET side lowers to
# `js_object_get_field_by_name_f64`, `js_object_get_field_ic_miss` and
# `js_typed_feedback_object_get_field_by_name_f64` (1324 / 532 / 556 calls in
# the same corpus), and not one of those three was in this set. So every
# property SET classified `MOVING: YES` and every property GET classified
# `MOVING: no`, which is the asymmetry that put "as of this writing all five
# violations the gate reports are `MOVING: YES via js_object_set_field_by_name`"
# in docs/src/internals/gc-rooting-invariant.md and left the GET half unread.
#
# Measured on the gate corpus at the commit that added these six names: 55
# `--stale-registers` uses have an emitted property-GET helper in their window
# and 31 of them classified `MOVING: no`, so `--moving-only` dropped all 31 —
# including the shape that faults deterministically under
# `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800` at zod's
# `clone`. The protector and the checker disagreed and the checker was wrong.
#
# The premise for each addition is the SAME premise the set already grants
# `js_object_get_field_by_name`: it runs a user getter through a transmuted
# function pointer (`get_field_by_name.rs:1064-1067`) and routes a registered
# Proxy to `js_proxy_get` (`get_field_by_name.rs:84`) — arbitrary JS, with its
# own back-edge polls. Each name below is an entry point that reaches it:
#
#   js_object_get_field_by_name_f64             ic_miss.rs:29  -> _by_name
#   js_object_get_field_by_name_boxed           ic_miss.rs:81,86 -> _f64
#   js_object_get_field_by_property_id_f64      ic_miss.rs:123 -> _f64
#   js_object_get_field_ic_miss                 ic_miss.rs:228,275 -> _by_name
#                                               ic_miss.rs:252 -> js_proxy_get
#   js_object_get_field_ic                      ic_miss.rs:526,561 -> _f64,
#                                               ic_miss.rs:544 -> _ic_miss
#   js_typed_feedback_object_get_field_by_name_f64
#                                               typed_feedback.rs:992 -> _f64
#
# `js_object_get_field_by_name` STAYS. It is a real exported symbol
# (`--audit-poll-capable` checks that), the runtime calls it internally, and a
# future lowering may emit it directly; removing it would be a narrowing bought
# for nothing. The bug was never that it was listed — it was that listing it
# read as covering the GET side when nothing in the corpus matched it.
#
# ## The ten names that were not symbols at all
#
# Auditing the set the way `--audit-alloc-re` audits ALLOC_RE found that TEN of
# its twenty-eight entries matched no `extern "C" fn js_*` anywhere in
# perry-runtime or perry-stdlib: `js_apply_function`, `js_array_for_each`,
# `js_array_sort`, `js_call_closure`, `js_call_value`, `js_function_call`,
# `js_invoke_closure`, `js_object_get_property`, `js_object_set_property`,
# `js_string_replace`. Extrapolated spellings, exactly like `regexp_alloc\w*`
# and the eight other dead ALLOC_RE alternatives, and with the same effect: the
# set LOOKED like it covered "call a JS closure" behind four separate entries
# and covered it zero times. The real closure-call entry points
# (`js_closure_callN`, 186 call sites in the gate corpus) were never in it, so
# a value held live across a plain JS function call classified `MOVING: no` —
# the most obviously poll-capable operation there is. `RECEIVER_SINKS` in this
# same file already spelled it `closure_call\w*`, so the correct name was three
# hundred lines away the whole time.
#
# Each phantom is replaced by the emitted symbol that carries the premise the
# phantom asserted, never deleted outright — dropping the fake name without the
# real one would keep the audit green and leave the hole:
#
#   js_call_closure / js_invoke_closure / js_function_call / js_apply_function
#       -> js_closure_call0..16, js_closure_call_array,
#          js_closure_call_apply_with_spread   (closure/dispatch/calln.rs:33 —
#          invokes a JS closure; nothing is more poll-capable than this)
#   js_call_value  -> js_native_call_value     (closure/dispatch/value_call.rs:17
#          — calls a value as a function, incl. the Proxy `apply` trap)
#   js_array_sort  -> js_array_sort_default, js_array_sort_with_comparator
#          (array/sort.rs:542,635 — the comparator is user JS)
#   js_array_for_each -> js_typed_array_for_each  (typedarray/iterate.rs:137)
#   js_object_get_property / js_object_set_property
#       -> js_object_get_property_key, js_object_set_property_key,
#          js_object_set_property_key_method  (object/property_key.rs:154,210,257
#          — ToPropertyKey coercion, then `js_object_get_field_by_name`)
#   js_string_replace -> the four `_fn` variants, which take a user replacer
#          callback (regex/replace_expand.rs:277 and siblings). The `_dyn` and
#          plain variants are a separate coverage decision with their own hit
#          count and are deliberately NOT folded in here — same reasoning as
#          ALLOC_RE's deleted `bigint_\w+_op`.
POLL_CAPABLE_RUNTIME = {
    "js_call_function",
    # Calling a JS closure. The four names this replaces
    # (`js_call_closure`, `js_invoke_closure`, `js_function_call`,
    # `js_apply_function`) were not symbols; these are.
    "js_closure_call0", "js_closure_call1", "js_closure_call2",
    "js_closure_call3", "js_closure_call4", "js_closure_call5",
    "js_closure_call6", "js_closure_call7", "js_closure_call8",
    "js_closure_call9", "js_closure_call10", "js_closure_call11",
    "js_closure_call12", "js_closure_call13", "js_closure_call14",
    "js_closure_call15", "js_closure_call16",
    "js_closure_call_array", "js_closure_call_apply_with_spread",
    "js_native_call_value",
    "js_object_get_property_key", "js_object_set_property_key",
    "js_object_set_property_key_method",
    "js_object_get_field_by_name", "js_object_set_field_by_name",
    # The property-GET dispatch codegen ACTUALLY emits. See the block comment
    # above for the reach proof behind each one.
    "js_object_get_field_by_name_f64", "js_object_get_field_by_name_boxed",
    "js_object_get_field_by_property_id_f64",
    "js_object_get_field_ic", "js_object_get_field_ic_miss",
    "js_typed_feedback_object_get_field_by_name_f64",
    "js_array_sort_default", "js_array_sort_with_comparator",
    "js_array_map", "js_array_filter", "js_typed_array_for_each",
    "js_array_reduce", "js_json_stringify",
    "js_string_replace_regex_fn", "js_string_replace_string_fn",
    "js_string_replace_all_regex_fn", "js_string_replace_all_string_fn",
    "js_promise_run_microtasks", "js_gc_loop_safepoint",
    # ToPrimitive / ToString / ToNumber: every one of these dispatches a user
    # `[Symbol.toPrimitive]` / `toString` / `valueOf` on an object operand.
    "js_to_primitive",
    "js_jsvalue_to_string", "js_jsvalue_to_string_coerce",
    "js_jsvalue_to_string_method", "js_jsvalue_to_string_radix",
    "js_string_coerce", "js_string_coerce_method_this",
    "js_number_coerce", "js_object_coerce",
}


def compute_poll_reaching(all_funcs):
    """Names of compiled functions that can transitively reach a moving-minor
    safepoint (`js_gc_loop_safepoint`) or a runtime helper that re-enters JS."""
    callees = {}
    for f in all_funcs:
        cs = set()
        for b in f.blocks:
            for ins in f.insns[b]:
                if ins.callee:
                    cs.add(ins.callee)
        callees[f.name] = cs
    polls = set()
    for name, cs in callees.items():
        if cs & POLL_CAPABLE_RUNTIME:
            polls.add(name)
    changed = True
    while changed:
        changed = False
        for name, cs in callees.items():
            if name in polls:
                continue
            if cs & polls:
                polls.add(name)
                changed = True
    return polls, set(callees)


# ------------------------------------------------------- dominance & windows

def dominators(f):
    """Cooper/Harvey/Kennedy iterative dominators. Returns idom map."""
    if not f.blocks:
        return {}
    entry = f.blocks[0]
    # reverse postorder
    order = []
    seen = set()

    def dfs(b):
        stack = [(b, iter(sorted(f.succs[b])))]
        seen.add(b)
        while stack:
            node, it = stack[-1]
            advanced = False
            for s in it:
                if s in f.insns and s not in seen:
                    seen.add(s)
                    stack.append((s, iter(sorted(f.succs[s]))))
                    advanced = True
                    break
            if not advanced:
                order.append(stack.pop()[0])

    dfs(entry)
    rpo = list(reversed(order))
    pos = {b: i for i, b in enumerate(rpo)}
    idom = {entry: entry}

    def intersect(a, b):
        while a != b:
            while pos[a] > pos[b]:
                a = idom[a]
            while pos[b] > pos[a]:
                b = idom[b]
        return a

    changed = True
    while changed:
        changed = False
        for b in rpo:
            if b == entry:
                continue
            new = None
            for p in f.preds[b]:
                if p not in pos or p not in idom:
                    continue
                new = p if new is None else intersect(p, new)
            if new is not None and idom.get(b) != new:
                idom[b] = new
                changed = True
    return idom


def dominates(idom, a, b):
    if a == b:
        return True
    cur = b
    while cur in idom and idom[cur] != cur:
        cur = idom[cur]
        if cur == a:
            return True
    return False


def between_blocks(f, a_blk, b_blk):
    """Blocks strictly between a_blk and b_blk on some path that does NOT
    re-enter a_blk (so a loop back-edge round trip is not counted -- that is a
    different dynamic instance of the value)."""
    if a_blk == b_blk:
        return set()
    fwd = set()
    q = deque(s for s in f.succs[a_blk] if s in f.insns and s != a_blk)
    while q:
        x = q.popleft()
        if x in fwd:
            continue
        fwd.add(x)
        if x == b_blk:
            continue          # sink: do not expand past the bind
        for s in f.succs[x]:
            if s in f.insns and s != a_blk:
                q.append(s)
    bwd = set()
    q = deque(p for p in f.preds[b_blk] if p in f.insns and p != a_blk)
    while q:
        x = q.popleft()
        if x in bwd:
            continue
        bwd.add(x)
        if x == b_blk:
            continue
        for p in f.preds[x]:
            if p in f.insns and p != a_blk:
                q.append(p)
    return (fwd & bwd) - {a_blk, b_blk}


# ---------------------------------------------------------- slot activity (must)

def must_active_slots(f):
    """Forward must-dataflow: which shadow slots are provably ACTIVE on entry
    to each block. gen = js_shadow_slot_bind(N,..), kill = js_shadow_slot_set(N,0)."""
    all_slots = set()
    for b in f.blocks:
        for ins in f.insns[b]:
            m = BIND_RE.search(ins.text)
            if m:
                all_slots.add(int(m.group(1)))
    if not all_slots:
        return {}
    entry = f.blocks[0] if f.blocks else None
    IN = {b: set(all_slots) for b in f.blocks}
    IN[entry] = set()
    changed = True
    while changed:
        changed = False
        for b in f.blocks:
            if b == entry:
                new = set()
            else:
                ps = [p for p in f.preds[b] if p in f.insns]
                if not ps:
                    new = set()
                else:
                    new = set(all_slots)
                    for p in ps:
                        new &= transfer(f, p, IN[p])
            if new != IN[b]:
                IN[b] = new
                changed = True
    return IN


def transfer(f, b, incoming):
    cur = set(incoming)
    for ins in f.insns[b]:
        m = BIND_RE.search(ins.text)
        if m:
            cur.add(int(m.group(1)))
            continue
        m = CLEAR_RE.search(ins.text)
        if m:
            cur.discard(int(m.group(1)))
    return cur


def active_at(f, IN, block, idx):
    cur = set(IN.get(block, set()))
    for ins in f.insns[block][:idx]:
        m = BIND_RE.search(ins.text)
        if m:
            cur.add(int(m.group(1)))
            continue
        m = CLEAR_RE.search(ins.text)
        if m:
            cur.discard(int(m.group(1)))
    return cur


# -------------------------------------------------------------- the check

class Violation:
    def __init__(self, module, func, alloc, store, bind, collectors, slot,
                 poll_reaching=frozenset()):
        self.module = module
        self.func = func
        self.alloc = alloc
        self.store = store
        self.bind = bind
        self.collectors = collectors
        self.slot = slot
        self.poll_reaching = poll_reaching

    @property
    def movers(self):
        return sorted({c.callee for c in self.collectors
                       if c.callee == MOVING_POLL or c.callee in self.poll_reaching
                       or c.callee in POLL_CAPABLE_RUNTIME})

    @property
    def moving(self):
        return bool(self.movers)

    @property
    def fingerprint(self):
        """Identity of this violation for allowlist matching.

        Deliberately built from names, never from SSA registers, slot indices
        or line numbers: `%12`, `slot 3` and "line 480" all move when an
        unrelated lowering above them changes by one instruction, and a
        fingerprint that churns is a fingerprint someone will replace with a
        numeric threshold. What stays put across a recompile is *which
        function*, *what it allocated*, and *what could collect before the
        root store landed* -- which is also exactly the triple a human needs
        to judge whether an entry is still the bug they signed off on.

        Coarser than one-per-instruction on purpose: two violations in the
        same function with the same alloc/collector pair collapse to one
        fingerprint, and the allowlist entry's `count` is what pins the
        multiplicity. A third one appearing later therefore still fails.
        """
        return "{}::{}::{}->{}".format(
            self.module, self.func, self.alloc.callee or "?",
            sorted({c.callee for c in self.collectors})[0]
            if self.collectors else "?",
        )


def check_func(module, f, want_moving_only=False, poll_reaching=frozenset(),
               anchor_mode="alloc"):
    if not f.blocks:
        return []
    order = {b: i for i, b in enumerate(f.blocks)}
    IN = must_active_slots(f)
    # map: alloca reg -> slot idx (from binds)
    slot_of_alloca = {}
    binds = []          # (Insn, slot, alloca)
    for b in f.blocks:
        for ins in f.insns[b]:
            m = BIND_RE.search(ins.text)
            if m:
                slot, alloca = int(m.group(1)), m.group(2)
                slot_of_alloca[alloca] = slot
                binds.append((ins, slot, alloca))

    if not binds:
        return []

    # index instructions by result register
    def_of = {}
    for b in f.blocks:
        for ins in f.insns[b]:
            if ins.result:
                def_of[ins.result] = ins

    idom = dominators(f)
    violations = []

    def window_hits(A, B):
        """Collecting calls on some CFG path from just after A to B."""
        hits = []
        if A.block == B.block:
            for c in f.insns[A.block]:
                if is_collecting(c.callee) and A.idx < c.idx < B.idx:
                    hits.append(c)
            return hits
        for c in f.insns[A.block]:
            if is_collecting(c.callee) and c.idx > A.idx:
                hits.append(c)
        for c in f.insns[B.block]:
            if is_collecting(c.callee) and c.idx < B.idx:
                hits.append(c)
        for m_blk in between_blocks(f, A.block, B.block):
            for c in f.insns[m_blk]:
                if is_collecting(c.callee):
                    hits.append(c)
        return hits

    def protected(A, B, chain):
        """Is the value rooted some other way inside the window?  A temp-root
        push or a mutable-capture box store of any register in the value's
        provenance chain roots it (both are scanned AND rewritten)."""
        def scan(blk, lo, hi):
            for c in f.insns[blk][lo:hi]:
                if c.callee in ROOTING_CALLS and uses(c.text, chain):
                    return True
            return False
        if A.block == B.block:
            return scan(A.block, A.idx + 1, B.idx)
        if scan(A.block, A.idx + 1, len(f.insns[A.block])):
            return True
        if scan(B.block, 0, B.idx):
            return True
        for m_blk in between_blocks(f, A.block, B.block):
            if scan(m_blk, 0, len(f.insns[m_blk])):
                return True
        return False

    for bind_ins, slot, alloca in binds:
        # The store this bind activates: nearest preceding store to `alloca`.
        store_ins = None
        for j in range(bind_ins.idx - 1, -1, -1):
            c = f.insns[bind_ins.block][j]
            sm = STORE_RE.match(c.text)
            if sm and sm.group(3) == alloca:
                store_ins = c
                break
        if store_ins is None:
            continue
        # Already-active slot bound to this alloca: the store itself publishes
        # the value through `bound_ptr`, so no window exists.
        if slot in active_at(f, IN, store_ins.block, store_ins.idx):
            continue
        val = STORE_RE.match(store_ins.text).group(2).strip()
        if not val.startswith("%"):
            continue
        reg = val[1:]
        chain = set()
        q = deque([reg])
        while q:
            r = q.popleft()
            if r in chain:
                continue
            chain.add(r)
            d = def_of.get(r)
            if d is not None and is_transparent(d):
                q.extend(operand_regs(d.text))
        for origin in provenance(def_of, reg):
            if anchor_mode == "alloc":
                if origin.callee is None or not ALLOC_RE.match(origin.callee):
                    continue
            else:
                # A load of a constant/global handle is not a materialization
                # worth anchoring in "any" mode either; keep calls only.
                if origin.callee is None or origin.callee in NONCOLLECTING:
                    continue
            # Real CFG dominance: the value bound must be the value this
            # instruction produced on every path reaching the bind.
            if not dominates(idom, origin.block, bind_ins.block):
                continue
            if origin.block == bind_ins.block and origin.idx >= bind_ins.idx:
                continue
            hits = window_hits(origin, bind_ins)
            if not hits:
                continue
            if protected(origin, bind_ins, chain):
                continue
            v = Violation(module, f.name, origin, store_ins, bind_ins, hits,
                          slot, poll_reaching)
            if want_moving_only and not v.moving:
                continue
            violations.append(v)
            break        # one report per bind: the widest window
    return violations


# -------------------------------------------------------------- allowlist ---
#
# Why a file of named entries and not `--max-violations N`:
#
# A numeric threshold cannot tell a new violation from an old one. Fix one bug,
# introduce a different one, and the count is unchanged and the gate is green --
# which is the failure mode this whole exercise exists to prevent. Worse, the
# number ratchets the wrong way under pressure: the cheapest way to make a red
# build green is to raise it by one, and nothing in the diff says what was
# conceded.
#
# So: one entry per known-remaining violation, each naming the issue that tracks
# it and carrying a written justification, and three properties that keep the
# file from decaying into a blanket suppression:
#
#   * an entry that matches NOTHING is an error. When the bug is fixed the entry
#     must be deleted in the same PR, so the allowlist cannot accumulate
#     tombstones that quietly widen its coverage later.
#   * an entry suppresses at most `count` violations. A second violation of the
#     same shape in the same function is new, and fails.
#   * a violation with no entry fails, regardless of how many entries exist.

ALLOWLIST_MIN_JUSTIFICATION = 40


class AllowlistError(Exception):
    """The allowlist itself is malformed, stale, or under-documented."""


class AllowEntry:
    __slots__ = ("fingerprint", "count", "issue", "justification", "matched")

    def __init__(self, fingerprint, count, issue, justification):
        self.fingerprint = fingerprint
        self.count = count
        self.issue = issue
        self.justification = justification
        self.matched = 0


def load_allowlist(path):
    """Parse and VALIDATE the allowlist. Every failure here is exit 2.

    Validation is strict because an allowlist is the one part of a gate whose
    whole purpose is to make it not fail; a typo'd field that silently parses
    to "suppress everything" would be indistinguishable from a working gate.
    """
    try:
        with open(path, "r") as fh:
            doc = json.load(fh)
    except FileNotFoundError:
        raise AllowlistError(f"{path}: no such file")
    except json.JSONDecodeError as exc:
        raise AllowlistError(f"{path}: not valid JSON: {exc}")

    if not isinstance(doc, dict) or "entries" not in doc:
        raise AllowlistError(
            f"{path}: expected a JSON object with an \"entries\" array")
    raw = doc["entries"]
    if not isinstance(raw, list):
        raise AllowlistError(f"{path}: \"entries\" must be an array")

    entries = {}
    for i, e in enumerate(raw):
        where = f"{path}: entries[{i}]"
        if not isinstance(e, dict):
            raise AllowlistError(f"{where}: must be an object")
        for field in ("fingerprint", "issue", "justification"):
            if not isinstance(e.get(field), str) or not e[field].strip():
                raise AllowlistError(
                    f"{where}: \"{field}\" is required and must be a "
                    "non-empty string")
        count = e.get("count", 1)
        if not isinstance(count, int) or isinstance(count, bool) or count < 1:
            raise AllowlistError(
                f"{where}: \"count\" must be a positive integer (got {count!r})")
        # A justification of "known issue" is not a justification. The length
        # floor is crude, but it is the difference between a reviewer having to
        # read a sentence and having to go excavate the history themselves.
        if len(e["justification"].strip()) < ALLOWLIST_MIN_JUSTIFICATION:
            raise AllowlistError(
                f"{where}: \"justification\" must be at least "
                f"{ALLOWLIST_MIN_JUSTIFICATION} characters explaining WHY this "
                "violation is known-safe or known-tracked. Suppressing a "
                "GC-rooting violation without saying why is how the next one "
                "gets waved through.")
        fp = e["fingerprint"].strip()
        if fp in entries:
            raise AllowlistError(
                f"{where}: duplicate fingerprint {fp!r}; merge the two entries "
                "and set \"count\" instead, so the suppressed multiplicity is "
                "stated in one place")
        entries[fp] = AllowEntry(fp, count, e["issue"].strip(),
                                 e["justification"].strip())
    return entries


def apply_allowlist(violations, entries):
    """Split `violations` into (suppressed, remaining) and mark entries used.

    Over-quota violations land in `remaining`: an entry is a licence for a
    stated number of hits, not for a shape.
    """
    seen = defaultdict(int)
    suppressed, remaining = [], []
    for v in violations:
        fp = v.fingerprint
        entry = entries.get(fp)
        if entry is None:
            remaining.append(v)
            continue
        seen[fp] += 1
        if seen[fp] <= entry.count:
            entry.matched += 1
            suppressed.append(v)
        else:
            remaining.append(v)
    return suppressed, remaining


def stale_entries(entries):
    """Entries that matched nothing. Always an error -- see the header note."""
    return [e for e in entries.values() if e.matched == 0]


# --------------------------------------------------- seeded-violation proof ---
#
# `--self-test` proves the checker fires on HAND-WRITTEN IR. Necessary, not
# sufficient: the fixtures are frozen text, so they keep passing even if perry's
# emitted IR drifts to a shape the parser can no longer read -- at which point
# the checker reports a serene `violations: 0` over a corpus it is not actually
# analysing. That is hazard 4 (CLAUDE.md) wearing the self-test's clothes, and
# it is the failure mode this repo has hit most often.
#
# So the gate also seeds violations into the REAL corpus and requires every one
# to be caught. For each sampled site we take a value that IS correctly rooted
# today and splice a collection point into the gap between the allocation and
# its root store -- mechanically manufacturing #7192's exact shape out of code
# that is currently right:
#
#     %v = call ptr @js_object_alloc(...)      <-- real
#     call double @js_call_function(...)       <-- spliced in
#     store ptr %v, ptr %slot                  <-- real
#     call void @js_shadow_slot_bind(...)      <-- real
#
# Only the collection point is synthetic. The function, its CFG, the allocation,
# the value's provenance chain through `or`/`bitcast`, the alloca, the store and
# the bind are all perry's own output, so the exercise covers the parser, the
# CFG builder, the provenance walk and the dominance computation against IR as
# it is actually emitted. Splicing rather than reordering also means a site
# always exists wherever a rooted allocation exists, instead of depending on the
# scheduler happening to leave a collecting call in a convenient place.
#
# If perry's IR stops having rooted allocations in the shape this matches, the
# gate goes red for "I could not seed a violation" rather than green for "I
# found none".

_ANY_BIND_RE = re.compile(
    r"^\s*call void @js_shadow_slot_bind\(i32 (\d+), ptr %([\w.$]+)\)")
_ANY_STORE_RE = re.compile(
    r"^\s*store\s+[\w\[\]x* ]+?\s+%([\w.$]+),\s*ptr %([\w.$]+)")
_ANY_ASSIGN_RE = re.compile(r"^\s*%([\w.$]+)\s*=\s*(.*)$")
_ANY_CALL_RHS_RE = re.compile(r"^call\s+[^@]*@([\w.$]+)\(")

# The spliced collection point. `js_call_function` is in POLL_CAPABLE_RUNTIME,
# so the planted violation is classified MOVING and survives `--moving-only` --
# i.e. the proof is about the configuration the gate actually ships with, not a
# laxer one. The mutant only has to be parseable by this checker, never
# compiled, so no declaration is needed.
_SEED_CALL = "  %gcseed.probe = call double @js_call_function(double 0.000000e+00)"


def _block_defs(lines, lo, hi):
    """reg -> right-hand side, for definitions in lines[lo:hi]."""
    defs = {}
    for j in range(lo, hi):
        m = _ANY_ASSIGN_RE.match(lines[j])
        if m:
            defs[m.group(1)] = m.group(2)
    return defs


def _reaches_alloc(defs, reg, limit=32):
    """Does `reg` trace back to an allocation through transparent ops only?

    Mirrors `provenance()` deliberately: the seeded site has to be one the
    checker's own anchoring would accept, otherwise a non-report is correct
    behaviour being counted as a miss.
    """
    seen = set()
    q = deque([reg])
    while q and len(seen) < limit:
        r = q.popleft()
        if r in seen:
            continue
        seen.add(r)
        rhs = defs.get(r)
        if rhs is None:
            continue
        cm = _ANY_CALL_RHS_RE.match(rhs)
        if cm:
            if ALLOC_RE.match(cm.group(1)):
                return True
            continue
        if any(op in rhs for op in TRANSPARENT_OPS):
            q.extend(re.findall(r"%([\w.$]+)", rhs))
    return False


def _seed_sites(lines):
    """Yield line indices at which to splice a collection point.

    A site is a `store`/`js_shadow_slot_bind` pair whose stored value traces
    back to an allocation earlier in the same basic block, where:

      * the slot is bound exactly once in the enclosing function and never
        `js_shadow_slot_set`, so it cannot already be active at the store --
        an already-active slot publishes through `bound_ptr` and the checker
        correctly reports nothing;
      * nothing in the gap roots the value another way, which would likewise
        make a non-report correct.

    Both exclusions matter: without them the "misses" this test reports would
    be the checker behaving properly, and the first person to see a red run
    would learn to distrust it.
    """
    fn_lo, fn_hi = 0, 0
    block_start = 0
    fn_text = ""
    for i, line in enumerate(lines):
        if line.startswith("define "):
            fn_lo = i
            fn_hi = next((k for k in range(i + 1, len(lines))
                          if lines[k].startswith("}")), len(lines))
            fn_text = "\n".join(lines[fn_lo:fn_hi])
            block_start = i + 1
            continue
        if LABEL_RE.match(line) or line.startswith("}"):
            block_start = i + 1
            continue
        if i < fn_lo or i >= fn_hi:
            continue
        bm = _ANY_BIND_RE.match(line)
        if not bm:
            continue
        slot, alloca = bm.group(1), bm.group(2)
        if i == 0:
            continue
        sm = _ANY_STORE_RE.match(lines[i - 1])
        if not sm or sm.group(2) != alloca:
            continue
        # the slot must not already be live at the store
        if fn_text.count(f"js_shadow_slot_bind(i32 {slot}, ") != 1:
            continue
        if f"js_shadow_slot_set(i32 {slot}," in fn_text:
            continue
        defs = _block_defs(lines, block_start, i - 1)
        if not _reaches_alloc(defs, sm.group(1)):
            continue
        # a rooting call already in the block would protect the mutant
        if any(rc in lines[j] for j in range(block_start, i - 1)
               for rc in ROOTING_CALLS):
            continue
        yield i - 1


def _mutate(lines, at):
    """Splice a collection point immediately above line `at`."""
    return lines[:at] + [_SEED_CALL] + lines[at:]


def seeded_violation_test(paths, moving_only, anchor, want_sites, verbose=False):
    """Return 0 if every seeded violation was caught, non-zero otherwise."""
    caught = missed = sites = 0
    misses = []
    with tempfile.TemporaryDirectory() as td:
        mutant = os.path.join(td, "mutant.ll")
        for p in sorted(paths):
            if sites >= want_sites:
                break
            with open(p, "r", errors="replace") as fh:
                lines = fh.read().splitlines()
            try:
                base, _ = _scan([p], moving_only, anchor)
            except MalformedIR:
                continue
            for at in _seed_sites(lines):
                if sites >= want_sites:
                    break
                with open(mutant, "w") as fh:
                    fh.write("\n".join(_mutate(lines, at)) + "\n")
                try:
                    after, _ = _scan([mutant], moving_only, anchor)
                except MalformedIR:
                    continue
                sites += 1
                if len(after) > len(base):
                    caught += 1
                    if verbose:
                        print(f"  seeded {os.path.basename(p)}:{at + 1} -> caught")
                else:
                    missed += 1
                    misses.append(f"{p}:{at + 1}")

    print(f"=== seeded violations: {sites} planted, {caught} caught, "
          f"{missed} MISSED")
    if sites == 0:
        print("error: could not seed a single violation into the corpus. The "
              "mutator looks for a store/bind pair whose value traces back to "
              "an allocation in the same block; finding none means perry's "
              "emitted IR no longer has the shape this checker anchors on, so "
              "a clean verdict from it would be meaningless.", file=sys.stderr)
        return 2
    if sites < want_sites:
        print(f"error: only {sites} seed site(s) found, wanted {want_sites}. "
              "Too narrow a sample to claim the checker still fires.",
              file=sys.stderr)
        return 2
    if missed:
        print("error: the checker did NOT report these seeded violations:",
              file=sys.stderr)
        for m in misses[:20]:
            print(f"  {m}", file=sys.stderr)
        print("A planted collection point between an allocation and its root "
              "store went unreported. The checker is not reading this IR the "
              "way it thinks it is; a clean verdict from it means nothing "
              "until this is explained.", file=sys.stderr)
        return 1
    return 0


# ------------------------------------------------------------- self-test ---
#
# The gate's own "can it fail?" arm. `PLANTED` is the #7186 shape: the instance
# is materialized, a call that allocates runs, and only THEN is the slot bound.
# `CLEAN` is byte-identical with the store/bind pair hoisted above the call.

_SELFTEST_PLANTED = """\
define double @perry_fn_selftest__late(double %a) {
entry.0:
  %slot = alloca i64
  call void @js_shadow_frame_enter(i32 1)
  %obj = call ptr @js_object_alloc(i32 4)
  %ret = call double @js_call_function(double %a)
  store ptr %obj, ptr %slot
  call void @js_shadow_slot_bind(i32 0, ptr %slot)
  ret double %ret
}

define double @perry_fn_selftest__branchy(double %a, i1 %c) {
entry.0:
  %slot = alloca i64
  call void @js_shadow_frame_enter(i32 1)
  %arr = call ptr @js_array_alloc(i32 8)
  br i1 %c, label %if.then.1, label %if.merge.2

if.then.1:
  %poll = call double @js_gc_loop_safepoint(double %a)
  br label %if.merge.2

if.merge.2:
  store ptr %arr, ptr %slot
  call void @js_shadow_slot_bind(i32 1, ptr %slot)
  ret double %a
}
"""

_SELFTEST_CLEAN = """\
define double @perry_fn_selftest__early(double %a) {
entry.0:
  %slot = alloca i64
  call void @js_shadow_frame_enter(i32 1)
  %obj = call ptr @js_object_alloc(i32 4)
  store ptr %obj, ptr %slot
  call void @js_shadow_slot_bind(i32 0, ptr %slot)
  %ret = call double @js_call_function(double %a)
  ret double %ret
}
"""

_SELFTEST_MALFORMED = """\
define double @perry_fn_selftest__nolabel(double %a) {
  %slot = alloca i64
  %obj = call ptr @js_object_alloc(i32 4)
  %ret = call double @js_call_function(double %a)
  store ptr %obj, ptr %slot
  call void @js_shadow_slot_bind(i32 0, ptr %slot)
  ret double %ret
}
"""


# ------------------------------------------------- stale-register invariant
#
# The check above anchors on a shadow-slot BIND, so it can only see values that
# are eventually rooted. The residual #7154 offender is not one of those: the
# receiver of `inst.check(f())` is read out of a closure capture cell, kept in a
# bare register across `f()`, and never rooted at all. `--stale-registers`
# checks the more general invariant the module header states:
#
#   No register holding a GC pointer may be USED below a collection point.
#   After the last thing that can collect, every operand is either re-read from
#   a root the collector rewrote, or re-derived from immutable storage.
#
# A "heap-value source" is an instruction that yields a value which may be a GC
# pointer.  Two kinds:
#
#   * an ALLOCATION (ALLOC_RE) -- unambiguously a fresh heap object;
#   * a ROOT READ -- a load of a location the collector rewrites (a shadow-slot
#     alloca, a closure capture cell, a temp-root slot, a module global, a
#     mutable-capture box).  Reading one produces a register that is correct
#     *now* and stale after the next evacuation, which is precisely the
#     "property (2) without property (3)" failure `temp_root.rs` describes.
#
# `js_closure_get_capture_bits` is the load-bearing one and it is deliberately
# in NONCOLLECTING (closure/alloc.rs:463 is a raw slot read that cannot
# allocate) -- being non-collecting is exactly what makes it a root READ rather
# than a collection point.

# Loads of these globals are root reads: module-level variables live in
# `@perry_global_*` and are registered roots that evacuation rewrites.
GLOBAL_ROOT_RE = re.compile(r"@perry_global_[\w.$]+")

# Loads whose source is a location the collector REWRITES. A register holding
# one of these is stale below a collection point even though the value survives
# — property (2) without property (3), the module-header distinction.
#
# ## Why this lives here rather than only next to `--unrooted-allocas`
#
# It was defined twice, in effect: `--unrooted-allocas` had this pattern and
# used it, while `--stale-registers` had `GLOBAL_ROOT_RE` and knew about
# `@perry_global_*` alone. **The two modes disagreed about what a
# collector-rewritten load is, and the narrower one was wrong.**
#
# The cost of that disagreement is #7240: a string literal lowers to
# `load double, ptr @…_.str.N.handle`, and the handle global IS a registered
# root — `js_gc_register_global_root`, `codegen/string_pool.rs` — so the string
# is never swept, but an evacuating cycle REWRITES the global while a register
# loaded from it beforehand keeps the pre-move address. Measured at #7240's
# fault: the handle global held the post-move address, the register held the
# retired from-space one. `heap_source_kind` classified that load as nothing at
# all, so the register had no recognised source and no stale use could be
# attributed to it. Over #7240's own gap test the checker reported 24
# `--moving-only` stale uses at the offending call and named NEITHER of the two
# literals that actually faulted; the registry's `alerts.ts` call passes
# literals in both unprotected positions, so it reported nothing whatsoever.
#
# Unlike `js_implicit_this_set` (#7226) and `js_regexp_new` (#7227), this one
# could NOT be closed by adding a name to `ALLOC_RE` — the source is a `load`,
# not a `call`. It needs the source SET widened, which is why it waited for its
# own before/after rather than riding along with a fix.
#
# One definition, both modes, so the next reader cannot re-derive half of it.
REWRITTEN_LOAD_RE = re.compile(
    r"load\s+\S+,\s*ptr\s+@(?:"
    r"(?P<strhandle>[\w.$]*_\.str\.\d+\.handle)"   # string-literal handles
    r"|(?P<global>perry_global_[\w.$]+)"           # module-level variables
    r"|(?P<classkeys>perry_class_keys_[\w.$]+)"    # class keys (old-gen, C4b movable)
    r")"
)


def rewritten_load_kind(text):
    """Which collector-rewritten global does this load read, if any?

    Returns the source kind (`strhandle` / `global` / `classkeys`) so the
    `--stale-registers` breakdown can be triaged one population at a time —
    they have genuinely different verdicts, and lumping them under one label
    would hide that.
    """
    m = REWRITTEN_LOAD_RE.search(text)
    return None if m is None else m.lastgroup

# Calls that READ a collector-rewritten location into a register.
ROOT_READ_CALLS = {
    "js_closure_get_capture_bits",   # closure/alloc.rs:463 capture cell read
    "js_closure_get_capture_ptr",
    "js_gc_temp_root_get",           # a MUTABLE root: rewritten on evacuation
    "js_box_get_bits",               # box.rs mutable-capture cell read
    "js_implicit_this_get",          # object/this_binding.rs:160 thread-local
    "js_new_target_get",
    # object/this_binding.rs:159 -- `js_implicit_this_SET` is a swap, so its
    # RETURN value is a read of the same scanned mutable cell
    # (`scan_implicit_this_roots_mut`, this_binding.rs:176) and the swap has
    # already overwritten the only other copy. Listing the setter as a reader
    # looks odd, which is exactly why #7214 left `prev_this` unrooted for a
    # whole PR: the checker saw a call it knew could not collect, never
    # classified the result as a heap value, and reported nothing at either
    # end. Being non-collecting is what makes a call a root READ.
    "js_implicit_this_set",
    # `js_get_string_pointer_unified` is a candidate and is deliberately left
    # out for now: its result IS a raw heap address in a bare register, but it
    # is not in NONCOLLECTING (its SSO branch allocates), so classifying it as
    # a source would report the whole `unbox_str_handle` family in one go --
    # roughly forty sites across lower_string_method.rs alone. That is a real
    # population and it needs its own measured count and its own triage rather
    # than being folded into this change. #7213.
}

# Argument positions that make a stale pointer FATAL rather than merely wrong:
# the value is dereferenced as an object.  Used only for ranking, never to
# suppress -- the check stays one-sided.
RECEIVER_SINKS = re.compile(
    r"^js_("
    r"typed_feedback_native_call_method\w*|native_call_method\w*|"
    r"native_call_value|object_get_field\w*|object_set_field\w*|"
    r"object_get_property|object_set_property|put_value_set_dyn_ic|"
    r"get_value_dyn_ic|closure_call\w*|call_closure|call_function|"
    r"call_value|invoke_closure|apply_function|"
    r"array_\w+|map_\w+|set_\w+|typed_feedback_\w*call\w*|"
    # A stale RegExpHeader* is dereferenced immediately by both of these —
    # this is #7154's residual, and it faulted rather than merely answering
    # wrong, so it belongs in the fatal ranking and not just the raw count.
    r"regexp_test|regexp_exec|regexp_match\w*|regexp_replace\w*"
    r")$"
)


def heap_source_kind(ins, slot_of_alloca):
    """Classify `ins` as a producer of a possibly-GC-pointer register."""
    if ins.result is None:
        return None
    if ins.callee is not None:
        if ALLOC_RE.match(ins.callee):
            return "alloc"
        if ins.callee in ROOT_READ_CALLS:
            return "capture" if "capture" in ins.callee else "rootread"
        return None
    if "= load " in ins.text:
        # `GLOBAL_ROOT_RE` first, and deliberately: it is the looser of the two
        # (it matches the name anywhere in the line, so it still catches a load
        # through a `getelementptr` on the global) and it keeps the `global`
        # population reporting under exactly the label it reported under
        # before. This widening is then strictly ADDITIVE — no previously
        # reported source changes kind, only sources that were invisible start
        # appearing — which is the property the gate's baselined counts need.
        if GLOBAL_ROOT_RE.search(ins.text):
            return "global"
        # #7240's third follow-up: a load of a string-literal handle global is
        # a heap-value source. See `REWRITTEN_LOAD_RE`.
        kind = rewritten_load_kind(ins.text)
        if kind is not None:
            return kind
        m = re.search(r"load\s+(?:i64|double)\s*,\s*ptr %([\w.$]+)", ins.text)
        if m and m.group(1) in slot_of_alloca:
            return "slotload"
    return None


class StaleUse:
    def __init__(self, module, func, src, kind, use, collectors, reg,
                 poll_reaching=frozenset()):
        self.module = module
        self.func = func
        self.src = src
        self.kind = kind
        self.use = use
        self.collectors = collectors
        self.reg = reg
        self.poll_reaching = poll_reaching

    @property
    def movers(self):
        return sorted({c.callee for c in self.collectors
                       if c.callee == MOVING_POLL or c.callee in self.poll_reaching
                       or c.callee in POLL_CAPABLE_RUNTIME})

    @property
    def moving(self):
        return bool(self.movers)

    @property
    def fatal_sink(self):
        return bool(self.use.callee and RECEIVER_SINKS.match(self.use.callee))


def check_func_stale(module, f, poll_reaching=frozenset(), moving_only=False):
    """Report registers holding a heap value that are USED below a collection
    point without being re-read from a root."""
    if not f.blocks:
        return []
    slot_of_alloca = {}
    for b in f.blocks:
        for ins in f.insns[b]:
            m = BIND_RE.search(ins.text)
            if m:
                slot_of_alloca[m.group(2)] = int(m.group(1))

    def_of = {}
    for b in f.blocks:
        for ins in f.insns[b]:
            if ins.result:
                def_of[ins.result] = ins

    idom = dominators(f)
    out = []

    for b in f.blocks:
        for src in f.insns[b]:
            kind = heap_source_kind(src, slot_of_alloca)
            if kind is None:
                continue
            # Forward closure over bit-level/identity ops: the untagged
            # pointer, the nanbox and the bitcast are all the same address.
            chain = {src.result}
            grew = True
            while grew:
                grew = False
                for bb in f.blocks:
                    for ins in f.insns[bb]:
                        if (ins.result and ins.result not in chain
                                and is_transparent(ins)
                                and uses(ins.text, chain)):
                            chain.add(ins.result)
                            grew = True
            # First real (non-transparent) use of any register in the chain
            # that sits below a collection point.
            for bb in f.blocks:
                if not dominates(idom, src.block, bb):
                    continue
                for use in f.insns[bb]:
                    if use.result in chain or is_transparent(use):
                        continue
                    if not uses(use.text, chain):
                        continue
                    if use.block == src.block and use.idx <= src.idx:
                        continue
                    # `js_shadow_slot_bind(N, ptr %alloca)` names the alloca,
                    # not the value; a store THROUGH the chain is a use of the
                    # pointer operand only when the chain is the stored value.
                    hits = window_hits_generic(f, src, use)
                    if not hits:
                        continue
                    v = StaleUse(module, f.name, src, kind, use, hits,
                                 src.result, poll_reaching)
                    if moving_only and not v.moving:
                        continue
                    out.append(v)
                    break
                else:
                    continue
                break
    return out


def window_hits_generic(f, A, B):
    """Collecting calls on some CFG path from just after A to just before B."""
    hits = []
    if A.block == B.block:
        for c in f.insns[A.block]:
            if is_collecting(c.callee) and A.idx < c.idx < B.idx:
                hits.append(c)
        return hits
    for c in f.insns[A.block]:
        if is_collecting(c.callee) and c.idx > A.idx:
            hits.append(c)
    for c in f.insns[B.block]:
        if is_collecting(c.callee) and c.idx < B.idx:
            hits.append(c)
    for m_blk in between_blocks(f, A.block, B.block):
        for c in f.insns[m_blk]:
            if is_collecting(c.callee):
                hits.append(c)
    return hits


def run_stale(parsed, poll_reaching, verbose, moving_only, fatal_only,
              max_stale=None):
    total = 0
    per_kind = defaultdict(int)
    per_sink = defaultdict(int)
    out = []
    for mod, fs in parsed:
        for f in fs:
            for v in check_func_stale(mod, f, poll_reaching, moving_only):
                if fatal_only and not v.fatal_sink:
                    continue
                total += 1
                per_kind[v.kind] += 1
                per_sink[v.use.callee or "store"] += 1
                cs = sorted({c.callee for c in v.collectors})
                out.append(
                    f"{mod}::{f.name}\n"
                    f"  source ({v.kind}): {v.src.text.strip()}\n"
                    f"  stale use       : {v.use.text.strip()}\n"
                    f"  between         : {', '.join(cs[:6])}"
                    f"{'  (+%d more)' % (len(cs) - 6) if len(cs) > 6 else ''}\n"
                    f"  MOVING          : "
                    f"{('YES via ' + ', '.join(v.movers[:3])) if v.moving else 'no'}\n"
                )
    if verbose:
        print("\n".join(out))
    print(f"=== stale-register uses: {total}")
    for k, n in sorted(per_kind.items(), key=lambda kv: -kv[1]):
        print(f"  {n:6d}  source={k}")
    for k, n in sorted(per_sink.items(), key=lambda kv: -kv[1])[:15]:
        print(f"  {n:6d}  sink={k}")

    # This mode is a DIAGNOSTIC, and unlike the bind-anchored check it is not
    # calibrated to zero. The count is dominated by values the checker cannot
    # prove are pointers, and even the `--fatal-sinks` slice still carries a
    # known-unfixed class (`js_closure_call*`, #7154), so `!= 0` is the normal
    # state of a healthy tree. Exiting 1 on any hit would make this a check
    # that can never pass, which is the same failure as the four "a gate that
    # cannot fail" hazards in CLAUDE.md read backwards: every caller learns to
    # ignore the exit status, and the day it means something nobody is looking.
    # Ranked leads are the product. Gating is opt-in and explicit via
    # --max-stale, which is how a calibrated slice becomes a ratchet later.
    if max_stale is not None:
        if total > max_stale:
            print(f"error: {total} stale-register use(s), budget is "
                  f"{max_stale}. Lower the count or raise --max-stale "
                  "deliberately.", file=sys.stderr)
            return 1
        print(f"within budget: {total} <= {max_stale}")
    return 0



# ------------------------------------------------- unrooted-alloca check ---
#
# The third way the invariant breaks (#7202), and the one the bind-anchored
# check above is structurally blind to.
#
# #7184 was a root store whose slot index fell outside the pushed frame.
# #7192 was a root store emitted after a collection point. Both produce a
# `js_shadow_slot_bind` for the checker to anchor on. This one produces NONE:
# the value lives in a plain `alloca_entry` for its whole lifetime, so the
# collector neither marks nor rewrites it, and a scan that starts from binds
# reports the function clean while every load below the collection point names
# from-space.
#
# The shape, from `lower_call/new.rs`'s inline-constructor `this` slot:
#
#     %slot = alloca double                    ; never in a js_shadow_slot_bind
#     store double %inst, ptr %slot
#     %x  = call double @user_fn()             ; collects; the instance MOVES
#     %t  = load double, ptr %slot             ; from-space
#
# It reports an alloca when ALL of:
#   1. it is never an operand of `js_shadow_slot_bind` anywhere in the function
#      (a bind makes the collector rewrite it, which is the fix);
#   2. some store into it carries a value whose provenance is a heap-value
#      source that can MOVE or be RECLAIMED — an allocation, or a load of a
#      collector-rewritten location, minus the exemptions enumerated in
#      `IMMOVABLE_SOURCES` (#7210: being a location the collector rewrites is
#      not the same as naming an object that can move);
#   3. a collecting call sits on some CFG path between that store and a LOAD of
#      the alloca.
#
# One-sided in the same direction as the bind-anchored check: `NONCOLLECTING`
# is the only place a call is declared safe, so a missing entry costs a false
# positive and never a missed bug. It is reported separately from the
# bind-anchored count because its two populations are disjoint by construction.

# `REWRITTEN_LOAD_RE` — the loads whose source the collector rewrites — is
# defined once, up with `GLOBAL_ROOT_RE` and the rest of the heap-value source
# vocabulary, because `--stale-registers` needs the same answer this mode does.
# It used to live only here, and the two modes disagreeing about it is exactly
# what made #7240's string-literal argument invisible; see the comment there.

# Calls that MATERIALIZE a heap value (a superset of ALLOC_RE: anything that
# hands back an object the collector can move).
HEAP_SOURCE_CALLS = frozenset({
    "js_gc_temp_root_get", "js_shadow_slot_get", "js_closure_get_capture_bits",
    "js_box_get_bits", "js_implicit_this_get", "js_new_target_get",
    "js_static_this_resolve", "js_get_exception",
})


# ------------------------------------------------- movability vs rewritability
#
# #7210 measured this predicate and found ALL 66 of its `--moving-only` hits to
# be false positives, for one reason: it answered
#
#     "does the collector REWRITE this location?"
#
# when the question the report is about is
#
#     "can the OBJECT this register names go bad while it sits in unrooted
#      memory across a collection?"
#
# Those are different questions, and `@perry_class_keys_*` is exactly where
# they come apart. The GLOBAL is registered with `js_gc_register_global_root`
# (`perry-codegen/src/codegen/string_pool.rs:477`) — so yes, the collector
# rewrites it. But the ARRAY it names is allocated by
# `js_build_class_keys_array` through `js_array_alloc_with_length_longlived`
# (`perry-runtime/src/object/alloc.rs:320,337`), i.e. in the OLD arena, which
# the nursery copying minor never relocates. 64 of the 66 were that, and 2 were
# `js_box_alloc_bits`, which is `std::alloc::alloc` — not in the GC heap at all.
#
# A register naming an object can go bad in exactly two independent ways, and
# the exemptions below have to close BOTH or they are unsound:
#
#   MOVABLE      some shipped/tested collector configuration relocates the
#                object, so the held address becomes stale. Only this one is
#                what `--moving-only` is about.
#   RECLAIMABLE  the object is freed if nothing else keeps it reachable, so
#                holding it in storage the precise root walk never visits is a
#                premature sweep (#7230's staging-buffer defect). This is what
#                the default (non-`--moving-only`) mode additionally covers.
#
# An exemption asserts BOTH are false and says why. It is one-sided in the
# UNSAFE direction — a source not listed here keeps the old, conservative
# classification — so the cost of forgetting one is a false positive, never a
# missed bug.
#
# ★ Every premise below is machine-checked by `--audit-immovable-sources`
# (wired into the gc-root-dominance workflow), and every exemption has a CLI
# knob that turns it back OFF so the counterfactual is one flag away rather
# than tribal knowledge. Both are deliberate: an exemption justified only by a
# comment is the shape that rots, and #7210's two conditions were written down
# precisely so somebody could re-check them.

class ImmovableSource:
    """A heap-value source whose object can neither move nor be reclaimed.

    `probes` are (path, regex, must_match) triples over the repository. They
    are the PREMISES of the exemption, not decoration: if one stops holding,
    `--audit-immovable-sources` goes red and the exemption must be re-argued
    rather than silently continuing to suppress real hazards.
    """

    def __init__(self, key, label, knob, callees=frozenset(), load_re=None,
                 not_movable_because="", not_reclaimable_because="",
                 becomes_real_when="", probes=()):
        self.key = key
        self.label = label
        self.knob = knob
        self.callees = frozenset(callees)
        self.load_re = load_re
        self.not_movable_because = not_movable_because
        self.not_reclaimable_because = not_reclaimable_because
        self.becomes_real_when = becomes_real_when
        self.probes = tuple(probes)

    def matches(self, ins):
        if ins.callee is not None:
            return ins.callee in self.callees
        return bool(self.load_re and self.load_re.search(ins.text))


# `js_build_class_keys_array`'s body, which the first probe reads. rustfmt puts
# a top-level fn's closing brace at column 0, so "from the signature to the
# next line that is exactly `}`" is the body and nothing else.
def rust_fn_body(path, fn_name):
    """Source text of a top-level Rust fn, or None if it is not there."""
    try:
        with open(path, encoding="utf-8", errors="replace") as fh:
            lines = fh.read().splitlines()
    except OSError:
        return None
    start = None
    for i, line in enumerate(lines):
        if re.search(r"\bfn\s+" + re.escape(fn_name) + r"\b", line):
            start = i
            break
    if start is None:
        return None
    for j in range(start + 1, len(lines)):
        if lines[j] == "}":
            return "\n".join(lines[start:j + 1])
    return None


def _probe_class_keys_longlived():
    """Every array allocation in `js_build_class_keys_array` is `_longlived`."""
    body = rust_fn_body("crates/perry-runtime/src/object/alloc.rs",
                        "js_build_class_keys_array")
    if body is None:
        return (False, "js_build_class_keys_array not found in "
                       "crates/perry-runtime/src/object/alloc.rs")
    allocs = re.findall(r"js_array_alloc\w*", body)
    if not allocs:
        return (False, "js_build_class_keys_array no longer allocates its "
                       "array with a js_array_alloc* call; the arena it uses "
                       "must be re-established by hand")
    bad = [a for a in allocs if "longlived" not in a]
    if bad:
        return (False, "js_build_class_keys_array now calls %s — a NURSERY "
                       "allocator. The class-keys exemption is void; the 64 "
                       "#7210 reports are real." % ", ".join(sorted(set(bad))))
    return (True, "%d longlived allocation(s)" % len(allocs))


def _probe_old_defrag_off_by_default():
    """Old-page defrag still returns an empty selection unless opted in."""
    try:
        with open("crates/perry-runtime/src/gc/oldgen.rs",
                  encoding="utf-8", errors="replace") as fh:
            src = fh.read()
    except OSError:
        return (False, "crates/perry-runtime/src/gc/oldgen.rs not readable")
    if "PERRY_GC_OLD_DEFRAG" not in src:
        return (False, "gc/oldgen.rs no longer mentions PERRY_GC_OLD_DEFRAG; "
                       "old-page defrag may now be unconditional")
    body = rust_fn_body("crates/perry-runtime/src/gc/oldgen.rs",
                        "select_old_page_defrag_pages")
    if body is None:
        return (False, "select_old_page_defrag_pages not found in gc/oldgen.rs")
    if not re.search(r"if\s+!old_page_defrag_enabled\(\)\s*\{\s*\n\s*return\s+"
                     r"OldPageDefragSelection::default\(\);", body):
        return (False, "select_old_page_defrag_pages no longer short-circuits "
                       "to an empty selection when defrag is disabled — "
                       "old-arena objects may relocate on a default build")
    return (True, "gated on PERRY_GC_OLD_DEFRAG, empty selection when off")


def _probe_class_keys_global_is_a_root():
    """The keys global is registered, so the array is never unreachable."""
    try:
        with open("crates/perry-codegen/src/codegen/string_pool.rs",
                  encoding="utf-8", errors="replace") as fh:
            src = fh.read()
    except OSError:
        return (False, "perry-codegen/src/codegen/string_pool.rs not readable")
    if "js_build_class_keys_array" not in src:
        return (False, "string_pool.rs no longer emits js_build_class_keys_array")
    if "js_gc_register_global_root" not in src:
        return (False, "string_pool.rs no longer registers the class-keys "
                       "global as a GC root — the array can now be swept")
    return (True, "@perry_class_keys_* registered via js_gc_register_global_root")


def _probe_boxes_outside_the_gc_heap():
    """Boxes are `std::alloc::alloc`, never freed, never arena-allocated."""
    try:
        with open("crates/perry-runtime/src/box.rs",
                  encoding="utf-8", errors="replace") as fh:
            src = fh.read()
    except OSError:
        return (False, "crates/perry-runtime/src/box.rs not readable")
    body = rust_fn_body("crates/perry-runtime/src/box.rs", "js_box_alloc_bits")
    if body is None:
        return (False, "js_box_alloc_bits not found in box.rs")
    if "alloc(layout)" not in body:
        return (False, "js_box_alloc_bits no longer allocates via "
                       "std::alloc::alloc; if it now uses arena_alloc_gc the "
                       "box IS a GC object and the exemption is void")
    if re.search(r"arena_alloc\w*\s*\(", src):
        return (False, "box.rs now calls an arena allocator — boxes may be GC "
                       "heap objects, which makes them movable AND sweepable")
    if re.search(r"\bdealloc\s*\(", src):
        return (False, "box.rs now frees boxes; the never-reclaimed premise of "
                       "the exemption no longer holds")
    return (True, "std::alloc::alloc, no arena allocation, no dealloc")


IMMOVABLE_SOURCES = (
    ImmovableSource(
        key="class-keys",
        label="@perry_class_keys_* / js_build_class_keys_array (old arena)",
        knob="assume_old_defrag",
        callees={"js_build_class_keys_array"},
        load_re=re.compile(r"load\s+\S+,\s*ptr\s+@perry_class_keys_\w+"),
        not_movable_because=(
            "js_build_class_keys_array allocates through "
            "js_array_alloc_with_length_longlived — the OLD arena. The nursery "
            "copying minor relocates nursery objects only, and the one thing "
            "that relocates an old-arena object is old-page defrag, which "
            "select_old_page_defrag_pages short-circuits off (#6206). No "
            "shipped configuration and no gc_repsel_matrix arm sets "
            "PERRY_GC_OLD_DEFRAG=1."),
        not_reclaimable_because=(
            "the @perry_class_keys_* global is registered with "
            "js_gc_register_global_root at module init, so the array is "
            "reachable from a root for the life of the process and no sweep "
            "can free it. (#5042 registered the global for the DEFRAG rewrite; "
            "the reachability is the side effect that closes this half.)"),
        becomes_real_when=(
            "PERRY_GC_OLD_DEFRAG ships on by default, or "
            "js_build_class_keys_array stops using a _longlived allocator. "
            "Re-check with --assume-old-defrag; the fix is to bind "
            "entry_init_load_global's cache slot (expr::root_entry_alloca "
            "accepts a bare i64 heap word)."),
        probes=(
            ("class-keys array is old-arena", _probe_class_keys_longlived),
            ("old-page defrag is off by default", _probe_old_defrag_off_by_default),
            ("the keys global is a registered root",
             _probe_class_keys_global_is_a_root),
        ),
    ),
    ImmovableSource(
        key="box",
        label="js_box_alloc* (outside the GC heap)",
        knob="assume_boxes_in_gc_heap",
        callees={"js_box_alloc", "js_box_alloc_bits", "js_i32_box_alloc",
                 "js_bool_box_alloc"},
        not_movable_because=(
            "js_box_alloc_bits uses std::alloc::alloc, so the Box is not an "
            "arena object and no collector phase relocates it. What the "
            "collector does touch is the JSValue INSIDE the box, which "
            "scan_box_roots_mut rewrites in place — the box's own address "
            "never changes."),
        not_reclaimable_because=(
            "boxes are never freed: box.rs has no dealloc, and BOX_REGISTRY is "
            "monotonic per thread (box.rs:429 states this as an invariant that "
            "js_box_is_box's membership test depends on)."),
        becomes_real_when=(
            "boxes become GC-heap allocations (arena_alloc_gc) or box.rs grows "
            "a free path. Re-check with --assume-boxes-in-gc-heap."),
        probes=(("boxes are outside the GC heap and never freed",
                 _probe_boxes_outside_the_gc_heap),),
    ),
)


def audit_immovable_sources():
    """Exit status for `--audit-immovable-sources`. 0 clean, 2 on a stale premise.

    Every `IMMOVABLE_SOURCES` entry SUPPRESSES reports. An exemption whose
    premise has quietly stopped holding is strictly worse than no exemption:
    it reads as a triaged false positive and is a live hazard. So the premises
    are a gate rather than a paragraph.
    """
    if not os.path.isdir("crates/perry-runtime/src"):
        print("error: run --audit-immovable-sources from the repository root "
              "(crates/perry-runtime/src not found).", file=sys.stderr)
        return 2
    if not IMMOVABLE_SOURCES:
        print("error: IMMOVABLE_SOURCES is empty, so this audit checks "
              "nothing. Delete the audit with the last exemption.",
              file=sys.stderr)
        return 2
    bad = 0
    n_probes = 0
    for src in IMMOVABLE_SOURCES:
        print(f"=== {src.key}: {src.label}")
        if not src.probes:
            print("  error: exemption has no probes — its premise is "
                  "unverifiable", file=sys.stderr)
            bad += 1
            continue
        for name, fn in src.probes:
            n_probes += 1
            ok, detail = fn()
            print(f"  [{'ok ' if ok else 'FAIL'}] {name}: {detail}")
            if not ok:
                bad += 1
    print(f"=== immovable-source premises: {n_probes} probe(s), {bad} failing")
    if bad:
        print("error: an exemption's premise no longer holds. The reports it "
              "was suppressing are real again — re-run with the exemption's "
              "knob (see --help) to see them.", file=sys.stderr)
        return 2
    return 0


def classify_heap_source(ins, exempt=True, assume_old_defrag=False,
                         assume_boxes_in_gc_heap=False):
    """`(is_heap_source, is_hazardous, exemption_or_None)` for one instruction.

    `is_hazardous` is the reportable property: the object can move, or can be
    reclaimed, while held in storage the precise root walk never visits. A
    source that is neither is a heap value the checker recognises and
    deliberately does not report.
    """
    if ins.callee is not None:
        is_source = bool(ALLOC_RE.match(ins.callee)) or ins.callee in HEAP_SOURCE_CALLS
    else:
        is_source = bool(REWRITTEN_LOAD_RE.search(ins.text))
    if not is_source:
        return (False, False, None)
    if not exempt:
        return (True, True, None)
    assumed = {"assume_old_defrag": assume_old_defrag,
               "assume_boxes_in_gc_heap": assume_boxes_in_gc_heap}
    for src in IMMOVABLE_SOURCES:
        if src.matches(ins):
            if assumed.get(src.knob):
                return (True, True, None)
            return (True, False, src)
    return (True, True, None)

ALLOCA_RE = re.compile(r"^\s*%([\w.$]+)\s*=\s*alloca\s+(.+)$")
LOAD_FROM_RE = re.compile(r"=\s*load\s+[^,]+,\s*ptr\s+%([\w.$]+)")
# Types that can hold a NaN-boxed JS value or a raw heap address. An `i32`/`i1`
# slot is a counter or a flag and cannot name the heap.
GC_CAPABLE_ALLOCA_TYPES = ("double", "i64", "[")


class UnrootedAlloca:
    def __init__(self, module, func, alloca, store, load, collectors,
                 poll_reaching=frozenset()):
        self.module = module
        self.func = func
        self.alloca = alloca
        self.store = store
        self.load = load
        self.collectors = collectors
        self.poll_reaching = poll_reaching

    @property
    def movers(self):
        return sorted({c.callee for c in self.collectors
                       if c.callee == MOVING_POLL or c.callee in self.poll_reaching
                       or c.callee in POLL_CAPABLE_RUNTIME})

    @property
    def moving(self):
        return bool(self.movers)


def _is_heap_source(ins, **kw):
    """Does `ins` materialize a value that can go bad in unrooted storage?

    Not "is this a heap value" and not "does the collector rewrite this
    location" — see the EXEMPTIONS block above for why conflating those cost
    #7210 sixty-six false positives.
    """
    _src, hazardous, _why = classify_heap_source(ins, **kw)
    return hazardous


def check_func_unrooted_allocas(module, f, want_moving_only=False,
                                poll_reaching=frozenset(), source_opts=None,
                                exempt_counts=None):
    source_opts = source_opts or {}
    if not f.blocks:
        return []

    bound = set()
    allocas = {}          # reg -> Insn
    def_of = {}
    for b in f.blocks:
        for ins in f.insns[b]:
            m = BIND_RE.search(ins.text)
            if m:
                bound.add(m.group(2))
            am = ALLOCA_RE.match(ins.text)
            if am and any(am.group(2).strip().startswith(t)
                          for t in GC_CAPABLE_ALLOCA_TYPES):
                allocas[am.group(1)] = ins
            if ins.result:
                def_of[ins.result] = ins
    if not allocas:
        return []

    # Alloca-typed registers that leak their ADDRESS to a call cannot be
    # reasoned about locally — the callee may root them. Exclude them rather
    # than report a guess.
    escaped = set()
    for b in f.blocks:
        for ins in f.insns[b]:
            if ins.callee is None:
                continue
            for r in operand_regs(ins.text):
                if r in allocas:
                    escaped.add(r)

    idom = dominators(f)
    stores = defaultdict(list)   # alloca -> [Insn]
    loads = defaultdict(list)    # alloca -> [Insn]
    for b in f.blocks:
        for ins in f.insns[b]:
            sm = STORE_RE.match(ins.text)
            if sm and sm.group(3) in allocas:
                stores[sm.group(3)].append(ins)
            lm = LOAD_FROM_RE.search(ins.text)
            if lm and lm.group(1) in allocas:
                loads[lm.group(1)].append(ins)

    def window_hits(A, B):
        hits = []
        if A.block == B.block:
            return [c for c in f.insns[A.block]
                    if is_collecting(c.callee) and A.idx < c.idx < B.idx]
        hits += [c for c in f.insns[A.block]
                 if is_collecting(c.callee) and c.idx > A.idx]
        hits += [c for c in f.insns[B.block]
                 if is_collecting(c.callee) and c.idx < B.idx]
        for m_blk in between_blocks(f, A.block, B.block):
            hits += [c for c in f.insns[m_blk] if is_collecting(c.callee)]
        return hits

    out = []
    for reg, alloca_ins in sorted(allocas.items()):
        if reg in bound or reg in escaped:
            continue
        if not stores[reg] or not loads[reg]:
            continue
        reported = False
        # Exemptions are TALLIED, never reported, and they must never
        # short-circuit the store loop: one alloca can be written from an
        # exempt source AND from a real one, and stopping at the exempt store
        # would turn the accounting into a missed hazard.
        exempt_seen = set()
        for st in stores[reg]:
            sm = STORE_RE.match(st.text)
            val = sm.group(2).strip()
            if not val.startswith("%"):
                continue        # a constant seed (`undefined`) names no heap
            origins = provenance(def_of, val[1:])
            hazardous = any(_is_heap_source(o, **source_opts) for o in origins)
            exemptions = []
            if not hazardous:
                # This site would have been reported before #7210's predicate
                # split. Record WHICH exemption dropped it, so a run that
                # reports zero can be told apart from a run that exempted the
                # whole corpus -- CLAUDE.md hazard 4, applied to the
                # suppression rather than to the check.
                for o in origins:
                    _s, _h, why = classify_heap_source(o, **source_opts)
                    if why is not None:
                        exemptions.append(why.key)
                if not exemptions:
                    continue
            for ld in loads[reg]:
                if not dominates(idom, st.block, ld.block):
                    continue
                if st.block == ld.block and st.idx >= ld.idx:
                    continue
                hits = window_hits(st, ld)
                if not hits:
                    continue
                v = UnrootedAlloca(module, f.name, alloca_ins, st, ld, hits,
                                   poll_reaching)
                if want_moving_only and not v.moving:
                    continue
                if hazardous:
                    out.append(v)
                    reported = True
                else:
                    exempt_seen.update(exemptions)
                break
            if reported:
                break
        if exempt_counts is not None:
            for k in sorted(exempt_seen):
                exempt_counts[k] += 1
    return out


# The #7202 shape, and its fix. `@unrooted` is `lower_call/new.rs`'s
# inline-constructor `this` slot before this change: allocated, stored, held
# across a user call, loaded after. `@rooted` is byte-identical with the bind
# added — which is the whole fix, because the collector then rewrites the
# alloca in place and the load below the call is correct.
_SELFTEST_UNROOTED = """\
define double @perry_fn_selftest__unrooted(double %a) {
entry.0:
  %slot = alloca double
  %inst = call i64 @js_object_alloc_class_inline_keys(i32 1, i32 0, i32 3, i64 0)
  %box = bitcast i64 %inst to double
  store double %box, ptr %slot
  %ret = call double @perry_fn_user__init(double %a)
  %this = load double, ptr %slot
  ret double %this
}
"""

_SELFTEST_ROOTED = """\
define double @perry_fn_selftest__rooted(double %a) {
entry.0:
  %slot = alloca double
  %inst = call i64 @js_object_alloc_class_inline_keys(i32 1, i32 0, i32 3, i64 0)
  %box = bitcast i64 %inst to double
  store double %box, ptr %slot
  call void @js_shadow_slot_bind(i32 0, ptr %slot)
  %ret = call double @perry_fn_user__init(double %a)
  %this = load double, ptr %slot
  ret double %this
}
"""


# The two #7210 exemption shapes, verbatim in the emitted-IR form the triage
# classified. `@class_keys` is `codegen/function.rs`'s `entry_init_load_global`
# cache; `@boxed` is a `js_box_alloc_bits` result parked in a local slot. Both
# are held across a collection point with no bind — i.e. structurally identical
# to `@unrooted` above — and both are NOT hazards, for reasons about the
# allocator rather than about the code shape. They are fixtures so that the
# exemption is proven to fire, and proven to be exactly reversible.
_SELFTEST_EXEMPT_CLASS_KEYS = """\
define double @perry_fn_selftest__class_keys(double %a) {
entry.0:
  %slot = alloca i64
  %keys = load i64, ptr @perry_class_keys_Foo_0
  store i64 %keys, ptr %slot
  %ret = call double @js_gc_loop_safepoint(double %a)
  %k = load i64, ptr %slot
  %obj = call i64 @js_object_alloc_class_inline_keys(i32 1, i32 0, i32 3, i64 %k)
  %box = bitcast i64 %obj to double
  ret double %box
}
"""

_SELFTEST_EXEMPT_BOX = """\
define double @perry_fn_selftest__box(double %a) {
entry.0:
  %slot = alloca i64
  %b = call i64 @js_box_alloc_bits(i64 0)
  store i64 %b, ptr %slot
  %ret = call double @js_gc_loop_safepoint(double %a)
  %p = load i64, ptr %slot
  %v = call i64 @js_box_get_bits(i64 %p)
  %box = bitcast i64 %v to double
  ret double %box
}
"""

# ★ The planted GENUINE hazard that keeps the exemptions honest. Same function
# shape as `@class_keys` — an `i64` slot holding a raw array pointer, loaded
# below a user call and fed to `js_object_alloc_class_inline_keys` as `keys` —
# but the pointer comes from `js_array_alloc_with_length`, a NURSERY allocator.
# If a future widening of the exemptions ever suppresses this, the exemption has
# stopped being about the allocator and started being about the code shape,
# which is the failure this whole split exists to prevent.
_SELFTEST_NURSERY_HAZARD = """\
define double @perry_fn_selftest__nursery(double %a) {
entry.0:
  %slot = alloca i64
  %keys = call i64 @js_array_alloc_with_length(i32 3)
  store i64 %keys, ptr %slot
  %ret = call double @js_gc_loop_safepoint(double %a)
  %k = load i64, ptr %slot
  %obj = call i64 @js_object_alloc_class_inline_keys(i32 1, i32 0, i32 3, i64 %k)
  %box = bitcast i64 %obj to double
  ret double %box
}
"""

# One slot, written from an EXEMPT source and then from a real one. The
# exemption must tally and get out of the way; if tallying it also ends the
# per-alloca search, the nursery store below it is never examined and the
# accounting has become a missed hazard. That is a bug this file has already
# had once, in the change that introduced the tally.
_SELFTEST_EXEMPT_THEN_HAZARD = """\
define double @perry_fn_selftest__mixed(double %a, i1 %c) {
entry.0:
  %slot = alloca i64
  %keys = load i64, ptr @perry_class_keys_Foo_0
  store i64 %keys, ptr %slot
  %p1 = call double @js_gc_loop_safepoint(double %a)
  %k1 = load i64, ptr %slot
  %fresh = call i64 @js_array_alloc_with_length(i32 3)
  store i64 %fresh, ptr %slot
  %p2 = call double @js_gc_loop_safepoint(double %a)
  %k2 = load i64, ptr %slot
  %obj = call i64 @js_object_alloc_class_inline_keys(i32 1, i32 0, i32 3, i64 %k2)
  %box = bitcast i64 %obj to double
  ret double %box
}
"""


# #7240's blind spot, both directions. `%lit` is a load of a string-literal
# handle global — a registered root, so the string is never SWEPT, but an
# evacuating cycle rewrites the global and leaves this register naming
# from-space. It is then passed as a call argument below `js_call_function`,
# which is exactly the registry's `defineApiCall(url, method, …)` shape.
#
# Before the widening `heap_source_kind` returned `None` for that load, so the
# register had no source and the mode reported ZERO over this fixture.
_SELFTEST_STR_HANDLE = """\
define double @perry_fn_selftest__strhandle(double %a) {
entry.0:
  %lit = load double, ptr @perry_mod_.str.3.handle
  %ret = call double @js_call_function(double %a)
  %r = call double @perry_fn_other__callee(double %lit, double %ret)
  ret double %r
}
"""

# The fix's shape: the load is re-emitted BELOW the collection point, so it
# observes the address evacuation wrote back. No runtime call, no temp root —
# `OperandProtection::Reload`.
_SELFTEST_STR_HANDLE_RELOADED = """\
define double @perry_fn_selftest__reload(double %a) {
entry.0:
  %ret = call double @js_call_function(double %a)
  %lit = load double, ptr @perry_mod_.str.3.handle
  %r = call double @perry_fn_other__callee(double %lit, double %ret)
  ret double %r
}
"""


# The property-GET window, both directions.
#
# This is the shape at which `--stale-registers` and
# `PERRY_GC_PROTECT_FROMSPACE=1 PERRY_GC_PROTECT_FROMSPACE_DEPTH=800`
# disagreed at zod's `clone(schema, def, params)`: the protector faulted
# deterministically, the checker classified the window `MOVING: no`, and
# `--moving-only` — the arm `gc-root-dominance.yml` gates on — dropped it. The
# checker was the wrong one. `POLL_CAPABLE_RUNTIME` held
# `js_object_get_field_by_name`, which codegen never emits, and none of the
# three GET entry points it DOES emit.
#
# Reduced from the real emitted IR (perry-codegen `property_get/
# generic_dispatch.rs`'s `pget.recv_ok` block, and the full-outline
# `js_object_get_field_ic` in `object/field_get_set/ic_miss.rs:544`):
# parameter #1 is spilled and bound, loaded into a bare register, and then a
# property read on the RECEIVER runs the generic GET dispatch — which resolves
# a user getter through a transmuted function pointer and can therefore run an
# evacuating minor — before that register is dereferenced.
#
# The bind is what makes this a `slotload` source rather than an invisible one:
# the slot IS rewritten by evacuation, so re-reading it is the fix and holding
# the pre-call register is the bug. Nothing in the fixture is an allocation, so
# the whole verdict rests on the window classification.
#
# The receiver and the key arrive as bare function PARAMETERS on purpose. That
# keeps the fixture to exactly one recognised source — `%def` — so the arms
# below assert one number about one register rather than a total that moves for
# any reason. It also records a second, separate gap honestly: `heap_source_kind`
# has no "incoming pointer parameter" kind, so `%a1` and `%a3` are invisible
# here and would be invisible in a real callee too. That gap is REAL but it is
# not what the zod adjudication turned on — measured over the gate corpus, only
# 4 unspilled parameters are used below a collecting call and all 4 are
# `double`/`i64` arithmetic in typed-f64 specialisations, none a pointer. It
# needs its own change with its own measured hit count.
_SELFTEST_PROPERTY_GET_WINDOW = """\
define double @perry_fn_selftest__clone(double %a1, double %a2, i64 %a3) {
entry.0:
  %d = alloca double
  store double %a2, ptr %d
  call void @js_shadow_slot_bind(i32 1, ptr %d)
  %def = load double, ptr %d
  %rb = bitcast double %a1 to i64
  %rh = and i64 %rb, 281474976710655
  call void @js_typed_feedback_observe_property_get(i64 4242, i64 %rh, i64 %a3)
  %inner = call double @js_object_get_field_ic_miss(i64 %rh, i64 %a3, ptr @perry_ic_1)
  %out = call double @perry_fn_other__callee(double %def, double %inner)
  ret double %out
}
"""

# The fix's shape: `%def` is re-read from its shadow slot BELOW the GET
# dispatch, so it observes the address evacuation wrote back. Identical in
# every other respect — if the control reports anything, the check is firing on
# the code shape rather than on the staleness.
_SELFTEST_PROPERTY_GET_RELOADED = """\
define double @perry_fn_selftest__clone_reloaded(double %a1, double %a2, i64 %a3) {
entry.0:
  %d = alloca double
  store double %a2, ptr %d
  call void @js_shadow_slot_bind(i32 1, ptr %d)
  %rb = bitcast double %a1 to i64
  %rh = and i64 %rb, 281474976710655
  call void @js_typed_feedback_observe_property_get(i64 4242, i64 %rh, i64 %a3)
  %inner = call double @js_object_get_field_ic_miss(i64 %rh, i64 %a3, ptr @perry_ic_1)
  %def = load double, ptr %d
  %out = call double @perry_fn_other__callee(double %def, double %inner)
  ret double %out
}
"""


def _scan_unrooted(paths, moving_only=False, **source_opts):
    """(violations, n_gc_capable_allocas) over `paths`."""
    parsed = [(os.path.basename(p), parse_file(p)) for p in sorted(paths)]
    poll_reaching, _known = compute_poll_reaching(
        [f for _m, fs in parsed for f in fs])
    n = 0
    for _m, fs in parsed:
        for f in fs:
            for b in f.blocks:
                for ins in f.insns[b]:
                    am = ALLOCA_RE.match(ins.text)
                    if am and any(am.group(2).strip().startswith(t)
                                  for t in GC_CAPABLE_ALLOCA_TYPES):
                        n += 1
    found = [
        (mod, v)
        for mod, fs in parsed
        for f in fs
        for v in check_func_unrooted_allocas(mod, f, moving_only, poll_reaching,
                                             source_opts)
    ]
    return found, n


def _scan(paths, moving_only, anchor):
    """(violations, n_binds) over `paths`."""
    parsed = [(os.path.basename(p), parse_file(p)) for p in sorted(paths)]
    poll_reaching, _known = compute_poll_reaching(
        [f for _m, fs in parsed for f in fs])
    binds = sum(
        1
        for _m, fs in parsed
        for f in fs
        for b in f.blocks
        for ins in f.insns[b]
        if BIND_RE.search(ins.text)
    )
    found = [
        v
        for mod, fs in parsed
        for f in fs
        for v in check_func(mod, f, moving_only, poll_reaching, anchor)
    ]
    return found, binds


def _stale_probe(path, max_stale):
    """(reported uses, exit status) from --stale-registers over one file."""
    parsed = [(os.path.basename(path), parse_file(path))]
    poll_reaching, _known = compute_poll_reaching(
        [f for _m, fs in parsed for f in fs])
    buf = io.StringIO()
    # Both streams: an over-budget probe is *expected* to print its error, and
    # that line in the self-test transcript reads like a real failure.
    with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
        rc = run_stale(parsed, poll_reaching, False, False, False, max_stale)
    m = re.search(r"stale-register uses: (\d+)", buf.getvalue())
    return (int(m.group(1)) if m else -1), rc


def _stale_kinds_probe(path, moving_only=False):
    """{source kind: count} from --stale-registers over one file.

    The breakdown, not just the total: this widening's whole claim is that a
    specific SOURCE became visible, and a total can move for any reason.
    """
    parsed = [(os.path.basename(path), parse_file(path))]
    poll_reaching, _known = compute_poll_reaching(
        [f for _m, fs in parsed for f in fs])
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
        run_stale(parsed, poll_reaching, False, moving_only, False, None)
    return {k: int(n)
            for n, k in re.findall(r"(\d+)\s+source=(\w+)", buf.getvalue())}


def _main_probe(argv):
    """Exit status of a full `main()` run over `argv` (no `sys.exit`).

    The guards these arms cover live in `main()`'s argument handling rather
    than in a scannable function, and argparse reports a usage error by
    raising `SystemExit(2)`. Driving `main()` is the only way to assert them;
    calling the inner helpers would skip exactly the code under test.
    """
    saved = sys.argv
    buf = io.StringIO()
    try:
        sys.argv = ["gc_root_dominance_check.py"] + list(argv)
        with contextlib.redirect_stdout(buf), contextlib.redirect_stderr(buf):
            try:
                return main()
            except SystemExit as exc:
                return exc.code if isinstance(exc.code, int) else 1
    finally:
        sys.argv = saved


def self_test():
    """Assert the checker reports the planted violation and clears the control.

    Returns 0 on success. A gate that never demonstrates a failure is a gate
    that has not been shown to work.
    """
    ok = True
    with tempfile.TemporaryDirectory() as td:
        planted = os.path.join(td, "planted.ll")
        clean = os.path.join(td, "clean.ll")
        broken = os.path.join(td, "broken.ll")
        for p, text in (
            (planted, _SELFTEST_PLANTED),
            (clean, _SELFTEST_CLEAN),
            (broken, _SELFTEST_MALFORMED),
        ):
            with open(p, "w") as fh:
                fh.write(text)

        found, binds = _scan([planted], False, "alloc")
        if len(found) != 2:
            print(f"self-test FAIL: planted fixture -> {len(found)} violations, "
                  "expected 2 (same-block and cross-block forms)", file=sys.stderr)
            ok = False
        if binds != 2:
            print(f"self-test FAIL: planted fixture -> {binds} binds, expected 2",
                  file=sys.stderr)
            ok = False
        if ok and not all(v.moving for v in found):
            print("self-test FAIL: both planted violations reach a moving minor "
                  "(js_call_function / js_gc_loop_safepoint) and must be "
                  "classified MOVING", file=sys.stderr)
            ok = False

        found, binds = _scan([clean], False, "alloc")
        if found:
            print(f"self-test FAIL: control fixture -> {len(found)} violations, "
                  "expected 0", file=sys.stderr)
            ok = False
        if binds != 1:
            print(f"self-test FAIL: control fixture -> {binds} binds, expected 1",
                  file=sys.stderr)
            ok = False

        # --stale-registers is a diagnostic, so its exit status is asserted
        # from both ends: the default must NOT go red on a corpus that has
        # hits, and --max-stale must actually be able to. A budget nobody has
        # watched fail is a budget that has not been shown to work.
        n_planted, rc_planted = _stale_probe(planted, None)
        if n_planted != 2:
            print(f"self-test FAIL: --stale-registers over the planted fixture "
                  f"-> {n_planted} uses, expected 2", file=sys.stderr)
            ok = False
        if rc_planted != 0:
            print("self-test FAIL: --stale-registers without --max-stale must "
                  f"exit 0 (diagnostic), got {rc_planted}", file=sys.stderr)
            ok = False
        if _stale_probe(planted, 0)[1] != 1:
            print("self-test FAIL: --max-stale 0 over the planted fixture must "
                  "exit 1; the budget cannot fail", file=sys.stderr)
            ok = False
        if _stale_probe(planted, 2)[1] != 0:
            print("self-test FAIL: --max-stale 2 over the planted fixture must "
                  "exit 0; the budget is off by one", file=sys.stderr)
            ok = False
        if _stale_probe(clean, 0) != (0, 0):
            print("self-test FAIL: --max-stale 0 over the control fixture must "
                  "report 0 uses and exit 0", file=sys.stderr)
            ok = False

        # --- the stale mode's own subject-liveness assertion ----------------
        #
        # `--stale-registers` classifies a shadow-slot load as a heap-value
        # source by looking the alloca up in a map built from
        # `js_shadow_slot_bind`. A corpus with no binds therefore has almost no
        # sources, reports `total 0`, and is indistinguishable from a corpus
        # with no stale registers. `--min-binds` is what makes that case an
        # error, and before #7211 the stale path returned above the guard.
        # Both directions, so the guard cannot be "always 2".
        if _main_probe(["--stale-registers", "--min-binds", "50", planted]) != 2:
            print("self-test FAIL: --stale-registers over a corpus with fewer "
                  "than --min-binds root stores must exit 2. The scan's "
                  "shadow-slot sources come from those stores, so a clean "
                  "verdict over zero of them proves nothing.", file=sys.stderr)
            ok = False
        if _main_probe(["--stale-registers", "--min-binds", "2", planted]) != 0:
            print("self-test FAIL: --stale-registers over a corpus that MEETS "
                  "--min-binds must still exit 0 (diagnostic). The guard "
                  "rejects every corpus, which is a gate that always fails.",
                  file=sys.stderr)
            ok = False
        # `--min-funcs` is the BREADTH floor, and it used to be evaluated below
        # the mode branches -- so `--stale-registers` and `--unrooted-allocas`
        # both returned before reaching it and ran to a verdict over a corpus
        # too thin to have exercised anything. A documented liveness control
        # disarmed by a mode flag, inside the script whose job is to catch
        # exactly that. The planted fixture has 2 functions; `--min-funcs 50`
        # must be an error in every mode and `--min-funcs 2` must not be one in
        # any of them, so the guard can be neither skipped nor always-on.
        for mode in (["--stale-registers"], ["--unrooted-allocas"], []):
            label = mode[0] if mode else "(bind-anchored default)"
            if _main_probe(mode + ["--min-funcs", "50", "--min-binds", "1",
                                   planted]) != 2:
                print(f"self-test FAIL: {label} over a 2-function corpus must "
                      "exit 2 at --min-funcs 50. A mode that returns above the "
                      "breadth floor silently disables it.", file=sys.stderr)
                ok = False
        for mode, want in ((["--stale-registers"], 0),
                           (["--unrooted-allocas"], 0),
                           ([], 1)):
            label = mode[0] if mode else "(bind-anchored default)"
            if _main_probe(mode + ["--min-funcs", "2", "--min-binds", "1",
                                   planted]) != want:
                print(f"self-test FAIL: {label} over a corpus that MEETS "
                      f"--min-funcs must exit {want}, not be rejected by the "
                      "breadth floor. A guard that rejects every corpus is a "
                      "gate that always fails.", file=sys.stderr)
                ok = False
        # --- the ALLOC_RE alternative audit, both directions ----------------
        #
        # The auditor is itself a gate, so it gets the same treatment as the
        # rest: prove it reports a planted dead alternative AND clears a live
        # one. Driven off a synthetic symbol set so the arm does not depend on
        # the repository's current contents -- an arm that only passes because
        # today's runtime happens to export the right names would go quiet the
        # moment someone renamed a symbol, which is the case it exists for.
        live_alts = [a for a in alloc_re_alternatives()
                     if re.compile("^js_(?:%s)$" % a).match("js_object_alloc")
                     or re.compile("^js_(?:%s)$" % a).match("js_regexp_new")]
        if not live_alts:
            print("self-test FAIL: neither js_object_alloc nor js_regexp_new "
                  "matches any ALLOC_RE alternative; the audit arm below would "
                  "be testing nothing", file=sys.stderr)
            ok = False
        if dead_alloc_alternatives({"js_object_alloc", "js_regexp_new"}) == []:
            print("self-test FAIL: audited against a two-symbol table, almost "
                  "every ALLOC_RE alternative must be reported dead. An empty "
                  "result means the auditor cannot detect a dead alternative "
                  "at all.", file=sys.stderr)
            ok = False
        # And it must clear an alternative that IS covered: build a symbol set
        # from one probe per alternative, and require a clean verdict.
        synthetic = set()
        for a in alloc_re_alternatives():
            # `\w*` -> `x`, `\w+` -> `x`: a concrete name the alternative matches.
            synthetic.add("js_" + a.replace(r"\w*", "x").replace(r"\w+", "x"))
        residue = dead_alloc_alternatives(synthetic)
        if residue:
            print(f"self-test FAIL: the auditor reported {len(residue)} "
                  "alternative(s) dead against a symbol set synthesised from "
                  f"the alternatives themselves: {residue}. That is a bug in "
                  "the auditor, not in ALLOC_RE.", file=sys.stderr)
            ok = False
        # --- the POLL_CAPABLE_RUNTIME audit, both directions -----------------
        #
        # Same treatment as the ALLOC_RE auditor above, and for a sharper
        # reason: this set decides the MOVING classification, so a phantom
        # entry is a hole in the arm the CI gate actually runs. Ten of
        # twenty-eight entries were phantoms when the auditor was written, and
        # four of those ten were four different misspellings of "call a JS
        # closure". Driven off synthetic symbol tables so the arm does not pass
        # merely because today's runtime happens to export the right names.
        if dead_poll_capable({"js_gc_loop_safepoint"}) == []:
            print("self-test FAIL: audited against a one-symbol table, almost "
                  "every POLL_CAPABLE_RUNTIME entry must be reported dead. An "
                  "empty result means the auditor cannot detect a phantom "
                  "entry at all.", file=sys.stderr)
            ok = False
        if dead_poll_capable(set(POLL_CAPABLE_RUNTIME)) != []:
            print("self-test FAIL: the auditor reported entries dead against a "
                  "symbol table that is POLL_CAPABLE_RUNTIME itself. That is a "
                  "bug in the auditor, not in the set.", file=sys.stderr)
            ok = False
        if dead_poll_capable(set(POLL_CAPABLE_RUNTIME) |
                             {"js_extra"}) != []:
            print("self-test FAIL: a symbol table that is a strict SUPERSET of "
                  "the set must also come back clean; the auditor is asking "
                  "the wrong containment question.", file=sys.stderr)
            ok = False
        # And the property-GET family specifically: the entries this whole
        # change is about must be in the set, or the fixture arms below are
        # asserting something the classification no longer does.
        _pget_family = {"js_object_get_field_by_name_f64",
                        "js_object_get_field_ic_miss",
                        "js_typed_feedback_object_get_field_by_name_f64"}
        if not _pget_family <= POLL_CAPABLE_RUNTIME:
            print("self-test FAIL: the emitted property-GET dispatch "
                  f"{sorted(_pget_family - POLL_CAPABLE_RUNTIME)} must stay in "
                  "POLL_CAPABLE_RUNTIME. Codegen emits these and never emits "
                  "js_object_get_field_by_name, so dropping them puts the GET "
                  "half of property access back outside --moving-only.",
                  file=sys.stderr)
            ok = False

        # A knob that is silently ignored is a disarmed knob -- the same rule
        # `--max-stale` and `--fatal-sinks` already carry, read the other way.
        if _main_probe(["--stale-registers", "--any-def", planted]) != 2:
            print("self-test FAIL: --any-def with --stale-registers must be a "
                  "usage error; the stale scan never consults the anchor, so "
                  "accepting it promises a widening that never happens",
                  file=sys.stderr)
            ok = False
        if _main_probe(["--any-def", planted]) != 1:
            print("self-test FAIL: --any-def WITHOUT --stale-registers must "
                  "still run the bind-anchored check and report the planted "
                  "violations", file=sys.stderr)
            ok = False

        # --- the string-literal handle source, both directions --------------
        #
        # #7240 shipped a codegen fix its own checker could not see. The load
        # of a `__perry_init_strings_*` handle global is a heap-value SOURCE —
        # the global is a registered root that evacuation REWRITES, so a
        # register loaded from it beforehand names from-space. Asserted as a
        # named source rather than as a total, because a total moves for any
        # reason and the claim here is about one specific population.
        strh = os.path.join(td, "strhandle.ll")
        strh_fixed = os.path.join(td, "strhandle_reloaded.ll")
        for p, text in ((strh, _SELFTEST_STR_HANDLE),
                        (strh_fixed, _SELFTEST_STR_HANDLE_RELOADED)):
            with open(p, "w") as fh:
                fh.write(text)

        kinds = _stale_kinds_probe(strh)
        if kinds.get("strhandle", 0) != 1:
            print("self-test FAIL: --stale-registers must report the load of a "
                  "string-literal handle global as source=strhandle. Got "
                  f"{kinds!r}. That load is a registered root the collector "
                  "REWRITES; a register holding it is stale below a collection "
                  "point, and missing it is what made #7240 invisible.",
                  file=sys.stderr)
            ok = False
        # `--moving-only` is the mode `gc-root-dominance.yml` gates on. A source
        # the gate cannot classify as reaching a moving minor is a source the
        # gate cannot fail on, so the raw count alone proves nothing.
        moving_kinds = _stale_kinds_probe(strh, moving_only=True)
        if moving_kinds.get("strhandle", 0) != 1:
            print("self-test FAIL: the planted strhandle use reaches a moving "
                  "minor via js_call_function and must survive --moving-only. "
                  f"Got {moving_kinds!r}", file=sys.stderr)
            ok = False
        if _stale_kinds_probe(strh_fixed).get("strhandle", 0) != 0:
            print("self-test FAIL: re-loading the handle global BELOW the "
                  "collection point is the fix (OperandProtection::Reload), so "
                  "the control fixture must report no strhandle use. The check "
                  "reports every literal load and cannot tell fixed from "
                  "broken.", file=sys.stderr)
            ok = False

        # --- the property-GET window, both directions -----------------------
        #
        # The zod `clone` adjudication. `--stale-registers` said `MOVING: no`
        # and the from-space protector faulted deterministically on the same
        # site; the checker was wrong, because `POLL_CAPABLE_RUNTIME` named the
        # one GET symbol codegen never emits and none of the three it does.
        #
        # The RAW arm is not the interesting one and never was — the window
        # already reported, it just reported as non-moving, and `--moving-only`
        # is the arm `gc-root-dominance.yml` gates on. So both are asserted,
        # and the second is the one that was red.
        pget = os.path.join(td, "property_get_window.ll")
        pget_fixed = os.path.join(td, "property_get_reloaded.ll")
        for p, text in ((pget, _SELFTEST_PROPERTY_GET_WINDOW),
                        (pget_fixed, _SELFTEST_PROPERTY_GET_RELOADED)):
            with open(p, "w") as fh:
                fh.write(text)

        if _stale_kinds_probe(pget).get("slotload", 0) != 1:
            print("self-test FAIL: --stale-registers must report the shadow-slot "
                  "load held across the generic property-GET dispatch as "
                  f"source=slotload. Got {_stale_kinds_probe(pget)!r}.",
                  file=sys.stderr)
            ok = False
        pget_moving = _stale_kinds_probe(pget, moving_only=True)
        if pget_moving.get("slotload", 0) != 1:
            print("self-test FAIL: the generic property-GET dispatch resolves a "
                  "user getter (object/field_get_set/get_field_by_name.rs:1064) "
                  "and routes a Proxy to js_proxy_get (:84), so a register held "
                  "across it MUST classify MOVING and survive --moving-only. "
                  f"Got {pget_moving!r}. This is the shape that faults under "
                  "PERRY_GC_PROTECT_FROMSPACE_DEPTH=800 at zod's `clone` while "
                  "the checker called the window non-moving; a gate that cannot "
                  "see a live bug is the failure mode this whole file exists "
                  "to avoid.", file=sys.stderr)
            ok = False
        if _stale_kinds_probe(pget_fixed).get("slotload", 0) != 0:
            print("self-test FAIL: re-reading the shadow slot BELOW the GET "
                  "dispatch is the fix, so the control fixture must report no "
                  "slotload use. A non-zero count means the check fires on the "
                  "code shape rather than on the staleness.", file=sys.stderr)
            ok = False

        try:
            _scan([broken], False, "alloc")
        except MalformedIR:
            pass
        else:
            print("self-test FAIL: a function with no basic-block label must "
                  "raise MalformedIR, not parse to zero blocks and report clean",
                  file=sys.stderr)
            ok = False

        # --- the #7202 mode, both directions -------------------------------
        unrooted = os.path.join(td, "unrooted.ll")
        rooted = os.path.join(td, "rooted.ll")
        for p, text in ((unrooted, _SELFTEST_UNROOTED), (rooted, _SELFTEST_ROOTED)):
            with open(p, "w") as fh:
                fh.write(text)

        found, n_allocas = _scan_unrooted([unrooted])
        if len(found) != 1:
            print(f"self-test FAIL: unrooted-alloca fixture -> {len(found)} "
                  "violations, expected 1", file=sys.stderr)
            ok = False
        if n_allocas != 1:
            print(f"self-test FAIL: unrooted-alloca fixture -> {n_allocas} "
                  "gc-capable allocas, expected 1", file=sys.stderr)
            ok = False

        found, n_allocas = _scan_unrooted([rooted])
        if found:
            print(f"self-test FAIL: rooted control -> {len(found)} violations, "
                  "expected 0. The ONLY difference from the planted fixture is "
                  "the js_shadow_slot_bind, so a non-zero count here means the "
                  "check does not actually model the fix.", file=sys.stderr)
            ok = False
        if n_allocas != 1:
            print(f"self-test FAIL: rooted control -> {n_allocas} gc-capable "
                  "allocas, expected 1", file=sys.stderr)
            ok = False

        # And it must not fire on the bind-anchored fixtures, nor the reverse:
        # the two populations are disjoint by construction and a checker that
        # double-counts would make both numbers meaningless.
        found, _ = _scan_unrooted([planted])
        if found:
            print(f"self-test FAIL: the bind-anchored planted fixture has a "
                  f"bind for every alloca, so the unrooted check must report 0, "
                  f"got {len(found)}", file=sys.stderr)
            ok = False

        # --- movability vs rewritability (#7210), all four directions -------
        #
        # An exemption is a suppression, so it gets the strictest treatment in
        # the file: it must FIRE (or the 66 false positives come back), it must
        # be exactly REVERSIBLE by its knob (or the counterfactual in #7210 is
        # unrecheckable), it must not suppress a genuine nursery hazard of the
        # SAME SHAPE (or it has become a code-shape rule instead of an
        # allocator rule), and its premises must still hold in the tree.
        ck = os.path.join(td, "exempt_class_keys.ll")
        bx = os.path.join(td, "exempt_box.ll")
        nz = os.path.join(td, "nursery_hazard.ll")
        for p, text in ((ck, _SELFTEST_EXEMPT_CLASS_KEYS),
                        (bx, _SELFTEST_EXEMPT_BOX),
                        (nz, _SELFTEST_NURSERY_HAZARD)):
            with open(p, "w") as fh:
                fh.write(text)

        for path, knob, label in ((ck, "assume_old_defrag", "class-keys"),
                                  (bx, "assume_boxes_in_gc_heap", "box")):
            found, n_allocas = _scan_unrooted([path], moving_only=True)
            if found:
                print(f"self-test FAIL: the {label} exemption fixture must "
                      f"report 0 under --moving-only, got {len(found)}. That "
                      "is the arm #7198's promote-to-required clock depends on.",
                      file=sys.stderr)
                ok = False
            found, n_allocas = _scan_unrooted([path])
            if found:
                print(f"self-test FAIL: the {label} exemption fixture must "
                      f"report 0, got {len(found)}. _is_heap_source has gone "
                      "back to conflating 'the collector rewrites this "
                      "location' with 'this object can move' (#7210).",
                      file=sys.stderr)
                ok = False
            if n_allocas != 1:
                print(f"self-test FAIL: {label} fixture -> {n_allocas} "
                      "gc-capable allocas, expected 1. A fixture that stopped "
                      "parsing would clear for the wrong reason.",
                      file=sys.stderr)
                ok = False
            found, _ = _scan_unrooted([path], **{knob: True})
            if len(found) != 1:
                print(f"self-test FAIL: --{knob.replace('_', '-')} must make "
                      f"the {label} fixture report again (got {len(found)}). "
                      "#7210 recorded the exact condition under which these "
                      "become real hazards; a knob that cannot restore them is "
                      "a condition nobody can re-check.", file=sys.stderr)
                ok = False

        # ★ The planted genuine hazard. Structurally identical to the
        # class-keys fixture — same slot type, same load below the same call,
        # same consumer — differing only in the ALLOCATOR.
        found, n_allocas = _scan_unrooted([nz])
        if len(found) != 1:
            print(f"self-test FAIL: a NURSERY-allocated keys array in an "
                  f"unrooted alloca across a collection point must still be "
                  f"reported, got {len(found)}. The exemptions have started "
                  "suppressing by code shape rather than by allocator.",
                  file=sys.stderr)
            ok = False
        if ok and found and not found[0][1].moving:
            print("self-test FAIL: the planted nursery hazard must classify "
                  "as MOVING, or --moving-only would drop it and the "
                  "acceptance proof would be vacuous", file=sys.stderr)
            ok = False
        found, _ = _scan_unrooted([nz], assume_old_defrag=True,
                                  assume_boxes_in_gc_heap=True)
        if len(found) != 1:
            print("self-test FAIL: the planted nursery hazard must be reported "
                  "under BOTH knobs too — the knobs only ever widen",
                  file=sys.stderr)
            ok = False
        found, _ = _scan_unrooted([nz], moving_only=True)
        if len(found) != 1:
            print("self-test FAIL: the planted nursery hazard must survive "
                  "--moving-only. If it does not, '--unrooted-allocas "
                  "--moving-only reaches 0' would be a statement about the "
                  "filter rather than about the corpus.", file=sys.stderr)
            ok = False

        # One slot, exempt store first, real store second. The tally must not
        # end the search for the alloca.
        mixed = os.path.join(td, "exempt_then_hazard.ll")
        with open(mixed, "w") as fh:
            fh.write(_SELFTEST_EXEMPT_THEN_HAZARD)
        found, _ = _scan_unrooted([mixed], moving_only=True)
        if len(found) != 1:
            print("self-test FAIL: an alloca written from an exempt source AND "
                  f"from a nursery allocator must still be reported (got "
                  f"{len(found)}). Tallying the exemption must not consume the "
                  "alloca's report.", file=sys.stderr)
            ok = False

        # Every exemption must be reachable through its knob, or it is a dead
        # switch. Asserted structurally rather than per-entry so a new
        # exemption cannot be added without one.
        knobs = {"assume_old_defrag", "assume_boxes_in_gc_heap"}
        for src in IMMOVABLE_SOURCES:
            if src.knob not in knobs:
                print(f"self-test FAIL: exemption {src.key!r} declares knob "
                      f"{src.knob!r}, which classify_heap_source does not "
                      "read. An exemption with no off-switch cannot be "
                      "re-checked.", file=sys.stderr)
                ok = False
            if not src.probes:
                print(f"self-test FAIL: exemption {src.key!r} has no premise "
                      "probes, so --audit-immovable-sources cannot tell "
                      "whether it is still true", file=sys.stderr)
                ok = False
            if not (src.not_movable_because and src.not_reclaimable_because
                    and src.becomes_real_when):
                print(f"self-test FAIL: exemption {src.key!r} must state why "
                      "the object cannot MOVE, why it cannot be RECLAIMED, "
                      "and when that stops being true. Suppressing one of the "
                      "two hazards is not an exemption.", file=sys.stderr)
                ok = False
        # ---- the allowlist must not be able to absorb a new violation ------
        #
        # These arms exist because the allowlist is the only component whose
        # job is to stop the gate failing. Every one of them is a way it could
        # quietly become a blanket suppression.
        planted_hits, _ = _scan([planted], False, "alloc")
        fps = sorted({v.fingerprint for v in planted_hits})
        if len(fps) != 2:
            print(f"self-test FAIL: planted fixture -> {len(fps)} distinct "
                  "fingerprints, expected 2 (the two functions differ, so the "
                  "fingerprint must distinguish them)", file=sys.stderr)
            ok = False

        def _write_allowlist(entries):
            p = os.path.join(td, "allow.json")
            with open(p, "w") as fh:
                json.dump({"entries": entries}, fh)
            return p

        good_reason = ("tracked in the issue above; the receiver is rooted by "
                       "the caller one frame up, verified by hand")

        if len(fps) == 2:
            # 1. Full coverage suppresses.
            al = load_allowlist(_write_allowlist([
                {"fingerprint": fp, "issue": "#0000",
                 "justification": good_reason} for fp in fps]))
            _sup, rem = apply_allowlist(planted_hits, al)
            if rem or stale_entries(al):
                print("self-test FAIL: an allowlist naming every planted "
                      "fingerprint should suppress all of them and go stale on "
                      "none", file=sys.stderr)
                ok = False

            # 2. Partial coverage still fails -- the uncovered one is "new".
            al = load_allowlist(_write_allowlist([
                {"fingerprint": fps[0], "issue": "#0000",
                 "justification": good_reason}]))
            _sup, rem = apply_allowlist(planted_hits, al)
            if len(rem) != 1:
                print(f"self-test FAIL: one entry covering one of two "
                      f"violations left {len(rem)} unallowed, expected 1. An "
                      "allowlist that suppresses a violation it does not name "
                      "is a blanket suppression.", file=sys.stderr)
                ok = False

            # 3. `count` is a quota, not a licence for the shape. Feed the same
            #    violation twice against a count of 1 and the second must
            #    survive -- this is the "new violation while the total is
            #    unchanged" case that a numeric threshold cannot see.
            al = load_allowlist(_write_allowlist([
                {"fingerprint": fps[0], "issue": "#0000", "count": 1,
                 "justification": good_reason}]))
            same = [v for v in planted_hits if v.fingerprint == fps[0]]
            _sup, rem = apply_allowlist(same + same, al)
            if len(rem) != 1:
                print(f"self-test FAIL: count=1 against two hits of the same "
                      f"fingerprint left {len(rem)} unallowed, expected 1",
                      file=sys.stderr)
                ok = False

            # 4. An entry that matches nothing is an error, so a fixed bug's
            #    tombstone cannot linger and widen coverage later.
            al = load_allowlist(_write_allowlist([
                {"fingerprint": "nosuch::nosuch::js_object_alloc->js_x",
                 "issue": "#0000", "justification": good_reason}]))
            apply_allowlist(planted_hits, al)
            if not stale_entries(al):
                print("self-test FAIL: an allowlist entry matching nothing must "
                      "be reported stale", file=sys.stderr)
                ok = False

        # 5. Under-documented and malformed entries are refused outright.
        for bad, why in (
            ([{"fingerprint": "a::b::c->d", "issue": "#1",
               "justification": "known"}], "a too-short justification"),
            ([{"fingerprint": "a::b::c->d", "justification": good_reason}],
             "a missing issue"),
            ([{"fingerprint": "a::b::c->d", "issue": "#1", "count": 0,
               "justification": good_reason}], "count=0"),
            ([{"fingerprint": "a::b::c->d", "issue": "#1",
               "justification": good_reason},
              {"fingerprint": "a::b::c->d", "issue": "#2",
               "justification": good_reason}], "a duplicate fingerprint"),
        ):
            try:
                load_allowlist(_write_allowlist(bad))
            except AllowlistError:
                pass
            else:
                print(f"self-test FAIL: {why} must be rejected", file=sys.stderr)
                ok = False

        # ---- the corpus mutator must be able to manufacture a violation ----
        # (proves seeded_violation_test's machinery works even before it is
        #  pointed at real IR, so a CI run that finds no sites is a real signal
        #  about the IR rather than a broken mutator.)
        with open(clean, "r") as fh:
            clean_lines = fh.read().splitlines()
        sites = list(_seed_sites(clean_lines))
        if not sites:
            print("self-test FAIL: the mutator found no seed site in the clean "
                  "fixture, which is a rooted alloc followed by a collecting "
                  "call — the exact shape it exists to find", file=sys.stderr)
            ok = False
        else:
            mutant = os.path.join(td, "mutant.ll")
            with open(mutant, "w") as fh:
                fh.write("\n".join(_mutate(clean_lines, sites[0])) + "\n")
            got, _ = _scan([mutant], False, "alloc")
            if not got:
                print("self-test FAIL: splicing a collecting call above the "
                      "root store in the CLEAN fixture must produce a "
                      "violation; the mutator or the checker is broken",
                      file=sys.stderr)
                ok = False

    print("self-test OK" if ok else "self-test FAILED")
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(
        description=__doc__.splitlines()[0],
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    ap.add_argument("paths", nargs="*", help=".ll files or directories to scan")
    ap.add_argument("--moving-only", action="store_true",
                    help="keep only violations whose window reaches a moving minor")
    ap.add_argument("--any-def", action="store_true",
                    help="anchor on any call result, not just allocation intrinsics")
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--self-test", action="store_true",
                    help="run the built-in planted/clean fixtures and exit")
    ap.add_argument("--stale-registers", action="store_true",
                    help="check the general stale-register invariant instead of "
                         "the bind-anchored one: report every register holding a "
                         "GC value that is USED below a collection point without "
                         "being re-read from a root (#7154). Diagnostic: this "
                         "mode exits 0 and reports counts unless --max-stale "
                         "gives it a budget")
    ap.add_argument("--fatal-sinks", action="store_true",
                    help="with --stale-registers, keep only uses that "
                         "DEREFERENCE the stale value (a call receiver/callee), "
                         "where a relocation is fatal rather than merely wrong")
    ap.add_argument("--max-stale", type=int, default=None, metavar="N",
                    help="with --stale-registers, exit 1 when more than N uses "
                         "are reported. Without it the mode is a ranked lead "
                         "list, not a pass/fail number, so it exits 0.")
    ap.add_argument("--min-files", type=int, default=1, metavar="N",
                    help="fail unless at least N .ll files were scanned (default 1)")
    ap.add_argument("--min-binds", type=int, default=1, metavar="N",
                    help="fail unless at least N root stores were seen (default 1). "
                         "A clean verdict over zero root stores proves nothing.")
    ap.add_argument("--unrooted-allocas", action="store_true",
                    help="check the #7202 shape instead: a plain alloca that "
                         "holds a heap value across a collecting call and is "
                         "loaded below it, with no js_shadow_slot_bind anywhere. "
                         "Disjoint from the bind-anchored check by construction.")
    ap.add_argument("--min-funcs", type=int, default=1, metavar="N",
                    help="fail unless at least N functions were checked (default 1). "
                         "Files and binds can both look healthy while the corpus "
                         "is one module deep; this asserts breadth.")
    ap.add_argument("--allowlist", metavar="PATH",
                    help="JSON file of known-remaining violations, one entry each "
                         "with an issue and a written justification. An entry that "
                         "matches nothing is an error, and an entry suppresses at "
                         "most its `count` hits -- so a NEW violation fails even "
                         "when the total is below some previous number.")
    ap.add_argument("--seeded-violations", type=int, default=0, metavar="N",
                    help="after checking, plant N synthetic violations into the "
                         "real corpus IR and require every one to be reported. "
                         "Proves the checker can still fail against the IR perry "
                         "actually emits, not just against frozen fixtures.")
    ap.add_argument("--audit-alloc-re", action="store_true",
                    help="check every ALLOC_RE alternative against the runtime's "
                         "real `extern \"C\" fn js_*` symbols and fail on any "
                         "that matches nothing. A dead alternative reads as "
                         "coverage and is not -- nine of them have shipped in "
                         "this pattern across two rounds. Takes no corpus.")
    ap.add_argument("--audit-poll-capable", action="store_true",
                    help="check every POLL_CAPABLE_RUNTIME entry against the "
                         "runtime's real `extern \"C\" fn js_*` symbols and "
                         "fail on any that names nothing. This set decides the "
                         "MOVING classification the --moving-only gate runs on, "
                         "so a phantom entry is a hole the gate cannot fail "
                         "through -- ten of twenty-eight were phantoms when "
                         "this was added. Takes no corpus.")
    ap.add_argument("--audit-immovable-sources", action="store_true",
                    help="re-check the PREMISES of every --unrooted-allocas "
                         "exemption against the runtime source (#7210): the "
                         "class-keys array is still old-arena, old-page defrag "
                         "is still off by default, the keys global is still a "
                         "registered root, and boxes are still outside the GC "
                         "heap and never freed. An exemption whose premise has "
                         "quietly lapsed reads as a triaged false positive and "
                         "is a live hazard. Takes no corpus.")
    ap.add_argument("--assume-old-defrag", action="store_true",
                    help="treat old-arena objects (the @perry_class_keys_* "
                         "cache) as MOVABLE, i.e. answer #7210's first "
                         "counterfactual: what --unrooted-allocas reports if "
                         "PERRY_GC_OLD_DEFRAG=1 ever ships on. Widens only.")
    ap.add_argument("--assume-boxes-in-gc-heap", action="store_true",
                    help="treat js_box_alloc* results as GC-heap objects, i.e. "
                         "#7210's second counterfactual: what "
                         "--unrooted-allocas reports if boxes ever stop being "
                         "std::alloc::alloc allocations. Widens only.")
    ns = ap.parse_args()

    # Standalone static audits: they read the crates, not an IR corpus, so they
    # are checked before the corpus arguments are.
    if ns.audit_alloc_re:
        return audit_alloc_re()
    if ns.audit_poll_capable:
        return audit_poll_capable()
    if ns.audit_immovable_sources:
        return audit_immovable_sources()

    # A knob that is silently ignored is a disarmed knob: `--max-stale 0`
    # without `--stale-registers` would run the bind-anchored check and look
    # like it enforced a budget, and `--fatal-sinks` alone would look like it
    # narrowed a report it never reached. Refuse instead (argparse exits 2).
    if ns.max_stale is not None and not ns.stale_registers:
        ap.error("--max-stale requires --stale-registers")
    if ns.fatal_sinks and not ns.stale_registers:
        ap.error("--fatal-sinks requires --stale-registers")
    # Same rule read the other way. `--any-def` only selects the bind-anchored
    # check's ANCHOR (`anchor = "any" if ns.any_def else "alloc"`, below), and
    # the stale-register path never consults it -- it anchors on every
    # heap-value source by construction, so there is no narrower or wider
    # setting for it to pick. Passing both reads like "widen the stale scan"
    # and does nothing at all.
    if ns.any_def and ns.stale_registers:
        ap.error("--any-def has no effect with --stale-registers "
                 "(the stale scan already anchors on every heap-value source)")
    # Same rule: --stale-registers returns before the --unrooted-allocas block,
    # so passing both would run the stale scan and silently skip the alloca one
    # while the command line claims both.
    if ns.stale_registers and ns.unrooted_allocas:
        ap.error("--stale-registers and --unrooted-allocas are separate "
                 "passes; run them one at a time")
    # Same disarmed-knob rule. The exemptions live in the unrooted-alloca pass
    # only, so `--assume-old-defrag` on any other pass would read like a
    # widening and do nothing at all.
    for flag, val in (("--assume-old-defrag", ns.assume_old_defrag),
                      ("--assume-boxes-in-gc-heap", ns.assume_boxes_in_gc_heap)):
        if val and not ns.unrooted_allocas:
            ap.error(f"{flag} requires --unrooted-allocas (it only affects "
                     "that pass's exemptions)")

    if ns.self_test:
        return self_test()

    allowlist = {}
    if ns.allowlist:
        try:
            allowlist = load_allowlist(ns.allowlist)
        except AllowlistError as exc:
            print(f"error: {exc}", file=sys.stderr)
            return 2

    moving_only = ns.moving_only
    anchor = "any" if ns.any_def else "alloc"
    verbose = ns.verbose
    paths = []
    for a in ns.paths:
        if os.path.isdir(a):
            for root, _dirs, files in os.walk(a):
                for fn in files:
                    if fn.endswith(".ll"):
                        paths.append(os.path.join(root, fn))
        elif os.path.isfile(a):
            paths.append(a)
        else:
            print(f"error: no such file or directory: {a}", file=sys.stderr)
            return 2

    # An empty corpus is a misconfigured run, never a pass. `--trace llvm`
    # silently produces nothing for a failed compile, and `PERRY_SAVE_LL` is not
    # written for modules that codegen splits into units, so "the directory is
    # empty" is a routine outcome rather than an exotic one.
    if len(paths) < ns.min_files:
        print(f"error: scanned {len(paths)} .ll file(s), need at least "
              f"{ns.min_files}. Nothing was checked.", file=sys.stderr)
        return 2

    parsed = []
    for p in sorted(paths):
        parsed.append((os.path.basename(p), parse_file(p)))
    poll_reaching, _known = compute_poll_reaching(
        [f for _m, fs in parsed for f in fs])
    n_binds = sum(
        1
        for _m, fs in parsed
        for f in fs
        for b in f.blocks
        for ins in f.insns[b]
        if BIND_RE.search(ins.text)
    )

    # `--min-funcs` is a corpus-BREADTH floor that belongs to every mode, so it
    # is computed HERE, above the mode branches, and not below them.
    #
    # It used to be computed below. `--stale-registers` and `--unrooted-allocas`
    # both return before that point, so `--stale-registers --min-funcs 1200` ran
    # to a verdict over a two-function corpus and exited 0 -- a documented
    # liveness control silently disarmed by a mode flag, in the very script whose
    # job is to catch gates that cannot fail. Reproduced on the self-test
    # fixture (2 functions): stale mode and alloca mode both exited 0 at
    # `--min-funcs 50` while the default bind-anchored mode correctly exited 2.
    # Both modes consult it before returning now, and `self_test()` carries a
    # failing and a passing arm for each so it cannot regress silently.
    n_funcs = sum(len(fs) for _m, fs in parsed)

    def funcs_floor_violated():
        if n_funcs >= ns.min_funcs:
            return False
        print(f"error: checked {n_funcs} function(s), need at least "
              f"{ns.min_funcs}. The corpus compiled but is too thin to have "
              "exercised the lowerings this invariant runs through.",
              file=sys.stderr)
        return True

    if ns.stale_registers:
        # `--min-binds` is a CORPUS-sanity assertion, not a bind-anchored-check
        # detail, so it has to be honoured here too. `heap_source_kind` decides
        # a `slotload` is a heap-value source by looking the pointer up in
        # `slot_of_alloca`, and that map is built from the same `BIND_RE`
        # (`stale_uses_in_function`). Compile the corpus with
        # PERRY_INLINE_SHADOW_SLOT=1 (or with a broken `--trace llvm`) and
        # every shadow-slot source vanishes: `run_stale` reports `total 0`,
        # exits 0, and looks exactly like a corpus with no stale registers in
        # it. That is hazard 4 -- the gate runs but its subject never did.
        if n_binds < ns.min_binds:
            print(f"error: {n_binds} root store(s) in the corpus, need at "
                  f"least {ns.min_binds}. The stale-register scan derives its "
                  "shadow-slot sources from those stores, so a clean verdict "
                  "here means the IR was not the IR you think it is "
                  "(compile with PERRY_INLINE_SHADOW_SLOT=0).", file=sys.stderr)
            return 2
        # Breadth, same as the bind-anchored path applies below. A corpus can
        # carry plenty of root stores and still be one module deep.
        if funcs_floor_violated():
            return 2
        return run_stale(parsed, poll_reaching, verbose, moving_only,
                         ns.fatal_sinks, ns.max_stale)

    if ns.unrooted_allocas:
        total = 0
        moving_total = 0
        per_fn = defaultdict(int)
        out = []
        n_allocas = 0
        source_opts = {"assume_old_defrag": ns.assume_old_defrag,
                       "assume_boxes_in_gc_heap": ns.assume_boxes_in_gc_heap}
        exempt_counts = defaultdict(int)
        for mod, fs in parsed:
            for f in fs:
                for b in f.blocks:
                    for ins in f.insns[b]:
                        am = ALLOCA_RE.match(ins.text)
                        if am and any(am.group(2).strip().startswith(t)
                                      for t in GC_CAPABLE_ALLOCA_TYPES):
                            n_allocas += 1
                for v in check_func_unrooted_allocas(mod, f, moving_only,
                                                     poll_reaching,
                                                     source_opts,
                                                     exempt_counts):
                    total += 1
                    per_fn[v.func] += 1
                    if v.moving:
                        moving_total += 1
                    cs = sorted({c.callee for c in v.collectors})
                    out.append(
                        f"{mod}::{v.func}\n"
                        f"  alloca : {v.alloca.text.strip()}\n"
                        f"  store  : {v.store.text.strip()}\n"
                        f"  load   : {v.load.text.strip()}\n"
                        f"  between: {', '.join(cs[:8])}"
                        f"{'  (+%d more)' % (len(cs) - 8) if len(cs) > 8 else ''}\n"
                        f"  MOVING : {('YES via ' + ', '.join(v.movers[:3])) if v.moving else 'no'}\n"
                    )
        if verbose:
            print("\n".join(out))
        print(f"=== files: {len(paths)}   gc-capable allocas: {n_allocas}   "
              f"unrooted-alloca violations: {total}"
              f"   (moving-minor reachable: {moving_total})")
        for k, n in sorted(per_fn.items(), key=lambda kv: -kv[1])[:20]:
            print(f"  {n:6d}  {k}")
        # A zero verdict is only meaningful next to what was EXEMPTED. Without
        # this line, "the corpus is clean" and "the corpus is entirely
        # exempted" print identically -- and the exemptions are the newest and
        # least-weathered part of the check.
        if exempt_counts:
            print("=== suppressed by an IMMOVABLE_SOURCES exemption "
                  "(#7210: rewritten location, immovable object):")
            by_key = {s.key: s for s in IMMOVABLE_SOURCES}
            for k, n in sorted(exempt_counts.items(), key=lambda kv: -kv[1]):
                src = by_key.get(k)
                knob = f"  (--{src.knob.replace('_', '-')} re-reports these)" if src else ""
                print(f"  {n:6d}  {k}{knob}")
        else:
            print("=== suppressed by an IMMOVABLE_SOURCES exemption: none")
        # Liveness floor: the subject here is the alloca population, not the
        # bind population, so `--min-binds` would certify the wrong thing.
        if n_allocas < ns.min_binds:
            print(f"error: {n_allocas} gc-capable alloca(s) in the corpus, need "
                  f"at least {ns.min_binds}. Nothing was checked.", file=sys.stderr)
            return 2
        if funcs_floor_violated():
            return 2
        return 1 if total else 0

    found = []
    for mod, fs in parsed:
        for f in fs:
            found.extend(check_func(mod, f, moving_only, poll_reaching, anchor))

    suppressed, remaining = apply_allowlist(found, allowlist)

    def render(v):
        cs = sorted({c.callee for c in v.collectors})
        return (
            f"{v.module}::{v.func}\n"
            f"  alloc  : {v.alloc.text.strip()}\n"
            f"  store  : {v.store.text.strip()}\n"
            f"  bind   : slot {v.slot}  {v.bind.text.strip()}\n"
            f"  between: {', '.join(cs[:8])}"
            f"{'  (+%d more)' % (len(cs) - 8) if len(cs) > 8 else ''}\n"
            f"  MOVING : {('YES via ' + ', '.join(v.movers[:3])) if v.moving else 'no'}\n"
            f"  fingerprint: {v.fingerprint}\n"
        )

    total = len(found)
    moving_total = sum(1 for v in found if v.moving)
    per_kind = defaultdict(int)
    per_kind_moving = defaultdict(int)
    for v in found:
        per_kind[v.alloc.callee] += 1
        if v.moving:
            per_kind_moving[v.alloc.callee] += 1

    if verbose:
        print("\n".join(render(v) for v in found))

    # The breadth line CI reads. Printing functions and modules -- not just a
    # violation count -- is what makes a silently-empty or silently-shrunken run
    # visible in the log instead of indistinguishable from a clean one.
    print(f"=== checked {n_funcs} functions / {len(parsed)} modules "
          f"({len(paths)} .ll files, {n_binds} root stores)")
    print(f"=== violations: {total}   (moving-minor reachable: {moving_total})")
    if allowlist:
        print(f"=== allowlisted: {len(suppressed)} hit(s) across "
              f"{len(allowlist)} entr(y/ies);  unallowed: {len(remaining)}")
    for k, n in sorted(per_kind.items(), key=lambda kv: -kv[1]):
        print(f"  {n:6d}  ({per_kind_moving.get(k, 0):5d} moving)  {k}")

    # --- subject-liveness assertions, before any verdict is announced -------
    if n_binds < ns.min_binds:
        print(f"error: {n_binds} root store(s) in the corpus, need at least "
              f"{ns.min_binds}. The subject of this check never ran — a clean "
              "verdict here means the IR was not the IR you think it is "
              "(compile with PERRY_INLINE_SHADOW_SLOT=0).", file=sys.stderr)
        return 2
    if funcs_floor_violated():
        return 2

    # --- allowlist hygiene --------------------------------------------------
    #
    # Both reports are printed before either return. Fixing one violation and
    # introducing a different one in the same PR makes an entry stale AND
    # produces an uncovered violation; returning on the stale entry first would
    # print only the bookkeeping problem and hide the actual new bug -- the one
    # finding this gate exists for.
    stale = stale_entries(allowlist)
    if stale:
        print("error: allowlist entries matched nothing:", file=sys.stderr)
        for e in stale:
            print(f"  {e.fingerprint}  ({e.issue})", file=sys.stderr)
        print("Either the violation was fixed — delete the entry in the same "
              "PR, that is the ratchet — or the corpus shrank and no longer "
              "contains the module it names, which means this run checked less "
              "than it claims to.", file=sys.stderr)

    if remaining:
        if not verbose:
            print("\n".join(render(v) for v in remaining))
        print(f"error: {len(remaining)} GC root-dominance violation(s) not "
              "covered by the allowlist.", file=sys.stderr)
        print("A GC-managed value is live across a collection point without "
              "being reachable from a root. See docs/src/internals/"
              "gc-rooting-invariant.md. If this is genuinely known and tracked, "
              "add an entry with an issue and a justification — never a count "
              "bump.", file=sys.stderr)

    # A stale entry is the more specific diagnosis (the allowlist itself is
    # wrong), so it keeps its exit code where both fire.
    if stale:
        return 2
    if remaining:
        return 1

    # --- can this gate still fail? ------------------------------------------
    if ns.seeded_violations:
        rc = seeded_violation_test(paths, moving_only, anchor,
                                   ns.seeded_violations, verbose)
        if rc:
            return rc

    return 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except MalformedIR as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(2)
