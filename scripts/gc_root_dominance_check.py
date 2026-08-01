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
CALL_RE = re.compile(r"\bcall\s+[^@]*@([\w.$]+)\(")
BIND_RE = re.compile(r"call void @js_shadow_slot_bind\(i32 (\d+), ptr %([\w.$]+)\)")
CLEAR_RE = re.compile(r"call void @js_shadow_slot_set\(i32 (\d+), i64 0\)")
STORE_RE = re.compile(r"^\s*store\s+([\w\[\]x* ]+?)\s+([^,]+),\s*ptr %([\w.$]+)")
BR_UNCOND_RE = re.compile(r"^\s*br label %([\w.$]+)")
BR_COND_RE = re.compile(r"^\s*br i1 [^,]+, label %([\w.$]+), label %([\w.$]+)")
SWITCH_LABEL_RE = re.compile(r"label %([\w.$]+)")


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
}

# The single site where an evacuating (moving) minor runs.
MOVING_POLL = "js_gc_loop_safepoint"

# Result-producing calls that materialize a fresh GC object.
ALLOC_RE = re.compile(
    r"^js_("
    r"object_alloc\w*|array_alloc\w*|closure_alloc\w*|box_alloc\w*|"
    r"string_alloc\w*|string_concat\w*|string_coerce|string_from\w*|"
    r"map_alloc\w*|set_alloc\w*|promise_alloc\w*|bigint_alloc\w*|"
    r"typed_array_alloc\w*|buffer_alloc\w*|regexp_alloc\w*|"
    r"object_create\w*|array_from\w*|build_class_keys_array"
    r")$"
)

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
POLL_CAPABLE_RUNTIME = {
    "js_call_function", "js_call_closure", "js_invoke_closure",
    "js_call_value", "js_apply_function", "js_function_call",
    "js_object_get_property", "js_object_set_property",
    "js_object_get_field_by_name", "js_object_set_field_by_name",
    "js_array_sort", "js_array_map", "js_array_filter", "js_array_for_each",
    "js_array_reduce", "js_json_stringify", "js_string_replace",
    "js_promise_run_microtasks", "js_gc_loop_safepoint",
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

# Calls that READ a collector-rewritten location into a register.
ROOT_READ_CALLS = {
    "js_closure_get_capture_bits",   # closure/alloc.rs:463 capture cell read
    "js_closure_get_capture_ptr",
    "js_gc_temp_root_get",           # a MUTABLE root: rewritten on evacuation
    "js_box_get_bits",               # box.rs mutable-capture cell read
    "js_implicit_this_get",          # object/this_binding.rs:160 thread-local
    "js_new_target_get",
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
    r"array_\w+|map_\w+|set_\w+|typed_feedback_\w*call\w*"
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
        if GLOBAL_ROOT_RE.search(ins.text):
            return "global"
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
#      SOURCE — an allocation, or a load of a collector-rewritten location;
#   3. a collecting call sits on some CFG path between that store and a LOAD of
#      the alloca.
#
# One-sided in the same direction as the bind-anchored check: `NONCOLLECTING`
# is the only place a call is declared safe, so a missing entry costs a false
# positive and never a missed bug. It is reported separately from the
# bind-anchored count because its two populations are disjoint by construction.

# Loads whose source is a location the collector REWRITES. A register holding
# one of these is stale below a collection point even though the value survives
# — property (2) without property (3), the module-header distinction.
REWRITTEN_LOAD_RE = re.compile(
    r"load\s+\S+,\s*ptr\s+@(?:"
    r"\w*_\.str\.\d+\.handle"          # string-literal handle globals
    r"|perry_global_\w+"               # module-level variables
    r"|perry_class_keys_\w+"           # class keys arrays (old-gen, C4b movable)
    r")"
)

# Calls that MATERIALIZE a heap value (a superset of ALLOC_RE: anything that
# hands back an object the collector can move).
HEAP_SOURCE_CALLS = frozenset({
    "js_gc_temp_root_get", "js_shadow_slot_get", "js_closure_get_capture_bits",
    "js_box_get_bits", "js_implicit_this_get", "js_new_target_get",
    "js_static_this_resolve", "js_get_exception",
})

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


def _is_heap_source(ins):
    """Does `ins` materialize a value the collector can move?"""
    if ins.callee is not None:
        return bool(ALLOC_RE.match(ins.callee)) or ins.callee in HEAP_SOURCE_CALLS
    return bool(REWRITTEN_LOAD_RE.search(ins.text))


def check_func_unrooted_allocas(module, f, want_moving_only=False,
                                poll_reaching=frozenset()):
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
        for st in stores[reg]:
            sm = STORE_RE.match(st.text)
            val = sm.group(2).strip()
            if not val.startswith("%"):
                continue        # a constant seed (`undefined`) names no heap
            origins = provenance(def_of, val[1:])
            if not any(_is_heap_source(o) for o in origins):
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
                out.append(v)
                reported = True
                break
            if reported:
                break
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


def _scan_unrooted(paths):
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
        for v in check_func_unrooted_allocas(mod, f, False, poll_reaching)
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
        (mod, v)
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
        if ok and not all(v.moving for _m, v in found):
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
    ns = ap.parse_args()

    # A knob that is silently ignored is a disarmed knob: `--max-stale 0`
    # without `--stale-registers` would run the bind-anchored check and look
    # like it enforced a budget, and `--fatal-sinks` alone would look like it
    # narrowed a report it never reached. Refuse instead (argparse exits 2).
    if ns.max_stale is not None and not ns.stale_registers:
        ap.error("--max-stale requires --stale-registers")
    if ns.fatal_sinks and not ns.stale_registers:
        ap.error("--fatal-sinks requires --stale-registers")

    if ns.self_test:
        return self_test()

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
    if ns.stale_registers:
        return run_stale(parsed, poll_reaching, verbose, moving_only,
                         ns.fatal_sinks, ns.max_stale)
    n_binds = sum(
        1
        for _m, fs in parsed
        for f in fs
        for b in f.blocks
        for ins in f.insns[b]
        if BIND_RE.search(ins.text)
    )

    if ns.unrooted_allocas:
        total = 0
        moving_total = 0
        per_fn = defaultdict(int)
        out = []
        n_allocas = 0
        for mod, fs in parsed:
            for f in fs:
                for b in f.blocks:
                    for ins in f.insns[b]:
                        am = ALLOCA_RE.match(ins.text)
                        if am and any(am.group(2).strip().startswith(t)
                                      for t in GC_CAPABLE_ALLOCA_TYPES):
                            n_allocas += 1
                for v in check_func_unrooted_allocas(mod, f, moving_only,
                                                     poll_reaching):
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
        # Liveness floor: the subject here is the alloca population, not the
        # bind population, so `--min-binds` would certify the wrong thing.
        if n_allocas < ns.min_binds:
            print(f"error: {n_allocas} gc-capable alloca(s) in the corpus, need "
                  f"at least {ns.min_binds}. Nothing was checked.", file=sys.stderr)
            return 2
        return 1 if total else 0

    total = 0
    moving_total = 0
    per_kind = defaultdict(int)
    per_kind_moving = defaultdict(int)
    out = []
    for mod, fs in parsed:
        for f in fs:
            for v in check_func(mod, f, moving_only, poll_reaching, anchor):
                total += 1
                per_kind[v.alloc.callee] += 1
                if v.moving:
                    moving_total += 1
                    per_kind_moving[v.alloc.callee] += 1
                cs = sorted({c.callee for c in v.collectors})
                out.append(
                    f"{mod}::{v.func}\n"
                    f"  alloc  : {v.alloc.text.strip()}\n"
                    f"  store  : {v.store.text.strip()}\n"
                    f"  bind   : slot {v.slot}  {v.bind.text.strip()}\n"
                    f"  between: {', '.join(cs[:8])}"
                    f"{'  (+%d more)' % (len(cs) - 8) if len(cs) > 8 else ''}\n"
                    f"  MOVING : {('YES via ' + ', '.join(v.movers[:3])) if v.moving else 'no'}\n"
                )
    if verbose:
        print("\n".join(out))
    print(f"=== files: {len(paths)}   root stores: {n_binds}   violations: {total}"
          f"   (moving-minor reachable: {moving_total})")
    for k, n in sorted(per_kind.items(), key=lambda kv: -kv[1]):
        print(f"  {n:6d}  ({per_kind_moving.get(k, 0):5d} moving)  {k}")
    if n_binds < ns.min_binds:
        print(f"error: {n_binds} root store(s) in the corpus, need at least "
              f"{ns.min_binds}. The subject of this check never ran — a clean "
              "verdict here means the IR was not the IR you think it is "
              "(compile with PERRY_INLINE_SHADOW_SLOT=0).", file=sys.stderr)
        return 2
    return 1 if total else 0


if __name__ == "__main__":
    try:
        sys.exit(main())
    except MalformedIR as exc:
        print(f"error: {exc}", file=sys.stderr)
        sys.exit(2)
