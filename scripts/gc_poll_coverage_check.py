#!/usr/bin/env python3
"""Poll-coverage checker over emitted LLVM IR (RFC deferred collection S5).

Under the allocation-point invariant (D2) a collection that the allocator arms
waits for the next POLL. Allocation between two polls is bounded only by the
valve, so every kind of repetition that allocates must pass a poll:

* **loops** — every natural loop whose body may allocate must contain a poll
  (`js_gc_loop_safepoint`, or an entry poll of a function it calls is NOT
  enough: a non-recursive direct callee carries none);
* **recursion** — every recursive SCC of the module's direct call graph whose
  members may allocate must have an entry poll (`js_gc_entry_safepoint`) in
  some member.

"May allocate" is one-sided in the safe direction: any call the root-dominance
checker does not list as non-collecting counts, except a call codegen marked
`"gc-leaf-function"` and calls to module functions that provably allocate
nothing (a fixed point over the module's own call graph).

Reports every uncovered loop and SCC. `--max-uncovered-loops` /
`--max-uncovered-sccs` turn the counts into a ratchet (they may only go
down); `--self-test` proves the checker can fail.

Usage:
  gc_poll_coverage_check.py [--max-uncovered-loops N] [--max-uncovered-sccs N] PATH...
  gc_poll_coverage_check.py --self-test
"""

from __future__ import annotations

import argparse
import os
import sys
import tempfile
from collections import defaultdict
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import gc_root_dominance_check as dom  # noqa: E402

POLLS = {"js_gc_loop_safepoint", "js_gc_entry_safepoint", "js_gc_entry_safepoint_args"}
ENTRY_POLLS = {"js_gc_entry_safepoint", "js_gc_entry_safepoint_args"}


def collecting_calls(func, defined):
    """(external collecting callees, internal callees) of `func`."""
    external, internal = set(), set()
    for block in func.blocks:
        for ins in func.insns[block]:
            callee = ins.callee
            if callee is None or callee in POLLS:
                continue
            if '"gc-leaf-function"' in ins.text:
                continue
            if callee in defined:
                internal.add(callee)
            elif dom.is_collecting(callee):
                external.add(callee)
    return external, internal


def allocating_functions(funcs):
    """Module functions that may allocate: a direct collecting call, or a call
    to a module function that may (fixed point)."""
    defined = {f.name for f in funcs}
    edges = {}
    allocating = set()
    for f in funcs:
        external, internal = collecting_calls(f, defined)
        edges[f.name] = internal
        if external:
            allocating.add(f.name)
    callers = defaultdict(set)
    for caller, callees in edges.items():
        for callee in callees:
            callers[callee].add(caller)
    work = list(allocating)
    while work:
        callee = work.pop()
        for caller in callers[callee]:
            if caller not in allocating:
                allocating.add(caller)
                work.append(caller)
    return allocating, edges


def natural_loops(func, idom):
    """[(header, body_blocks)] for every back edge t -> h with h dominating t."""
    loops = []
    for tail in func.blocks:
        for head in func.succs[tail]:
            if head not in func.insns or not dom.dominates(idom, head, tail):
                continue
            body = {head, tail}
            stack = [tail]
            while stack:
                block = stack.pop()
                if block == head:
                    continue
                for pred in func.preds[block]:
                    if pred not in body:
                        body.add(pred)
                        stack.append(pred)
            loops.append((head, body))
    return loops


def loop_may_allocate(func, body, allocating, defined):
    for block in body:
        for ins in func.insns[block]:
            callee = ins.callee
            if callee is None or callee in POLLS or '"gc-leaf-function"' in ins.text:
                continue
            if callee in defined:
                if callee in allocating:
                    return True
            elif dom.is_collecting(callee):
                return True
    return False


def loop_polls(func, body):
    return any(
        ins.callee in POLLS for block in body for ins in func.insns[block]
    )


def recursive_sccs(edges):
    index, low, on_stack, stack, out = {}, {}, set(), [], []
    counter = [0]
    for root in sorted(edges):
        if root in index:
            continue
        work = [(root, iter(sorted(edges[root])))]
        index[root] = low[root] = counter[0]
        counter[0] += 1
        stack.append(root)
        on_stack.add(root)
        while work:
            node, it = work[-1]
            advanced = False
            for succ in it:
                if succ not in edges:
                    continue
                if succ not in index:
                    index[succ] = low[succ] = counter[0]
                    counter[0] += 1
                    stack.append(succ)
                    on_stack.add(succ)
                    work.append((succ, iter(sorted(edges[succ]))))
                    advanced = True
                    break
                if succ in on_stack:
                    low[node] = min(low[node], index[succ])
            if advanced:
                continue
            work.pop()
            if work:
                parent = work[-1][0]
                low[parent] = min(low[parent], low[node])
            if low[node] == index[node]:
                component = []
                while True:
                    member = stack.pop()
                    on_stack.discard(member)
                    component.append(member)
                    if member == node:
                        break
                if len(component) > 1 or node in edges[node]:
                    out.append(sorted(component))
    return out


def check(funcs):
    """(uncovered_loops, uncovered_sccs, totals) for one module's functions."""
    defined = {f.name for f in funcs}
    allocating, edges = allocating_functions(funcs)
    uncovered_loops, uncovered_sccs = [], []
    totals = {"loops": 0, "allocating_loops": 0, "sccs": 0, "allocating_sccs": 0}
    by_name = {f.name: f for f in funcs}
    for f in funcs:
        idom = dom.dominators(f)
        for head, body in natural_loops(f, idom):
            totals["loops"] += 1
            if not loop_may_allocate(f, body, allocating, defined):
                continue
            totals["allocating_loops"] += 1
            if not loop_polls(f, body):
                uncovered_loops.append(f"{f.name}: loop at %{head}")
    for scc in recursive_sccs(edges):
        totals["sccs"] += 1
        if not any(member in allocating for member in scc):
            continue
        totals["allocating_sccs"] += 1
        polled = any(
            ins.callee in ENTRY_POLLS
            for member in scc
            for block in by_name[member].blocks
            for ins in by_name[member].insns[block]
        )
        if not polled:
            uncovered_sccs.append(" -> ".join(scc))
    return uncovered_loops, uncovered_sccs, totals


def ll_files(paths):
    for path in paths:
        p = Path(path)
        if p.is_dir():
            yield from sorted(p.rglob("*.ll"))
        else:
            yield p


def run(paths, max_loops, max_sccs, verbose):
    all_loops, all_sccs = [], []
    totals = defaultdict(int)
    files = 0
    for path in ll_files(paths):
        files += 1
        funcs = dom.parse_file(str(path))
        loops, sccs, t = check(funcs)
        all_loops += [f"{path.name}: {entry}" for entry in loops]
        all_sccs += [f"{path.name}: {entry}" for entry in sccs]
        for key, value in t.items():
            totals[key] += value
    if files == 0:
        print("gc-poll-coverage: no .ll files — the check did not run", file=sys.stderr)
        return 1
    print(
        f"gc-poll-coverage: {files} file(s); loops={totals['loops']} "
        f"allocating={totals['allocating_loops']} uncovered={len(all_loops)}; "
        f"recursive_sccs={totals['sccs']} allocating={totals['allocating_sccs']} "
        f"uncovered={len(all_sccs)}"
    )
    shown = all_loops if verbose else all_loops[:20]
    for entry in shown:
        print(f"  uncovered loop: {entry}")
    for entry in all_sccs if verbose else all_sccs[:20]:
        print(f"  uncovered SCC: {entry}")
    failed = False
    if max_loops is not None and len(all_loops) > max_loops:
        print(f"gc-poll-coverage: FAIL: {len(all_loops)} uncovered loops > {max_loops}", file=sys.stderr)
        failed = True
    if max_sccs is not None and len(all_sccs) > max_sccs:
        print(f"gc-poll-coverage: FAIL: {len(all_sccs)} uncovered SCCs > {max_sccs}", file=sys.stderr)
        failed = True
    return 1 if failed else 0


SELF_TEST_IR = """
define double @polled_loop() {
entry.0:
  br label %loop.1
loop.1:
  %a = call i64 @js_object_alloc(i32 0, i32 0)
  call void @js_gc_loop_safepoint()
  br i1 true, label %loop.1, label %exit.2
exit.2:
  ret double 0.0
}

define double @bare_loop() {
entry.0:
  br label %loop.1
loop.1:
  %a = call i64 @js_object_alloc(i32 0, i32 0)
  br i1 true, label %loop.1, label %exit.2
exit.2:
  ret double 0.0
}

define double @pure_loop() {
entry.0:
  br label %loop.1
loop.1:
  br i1 true, label %loop.1, label %exit.2
exit.2:
  ret double 0.0
}

define double @rec_polled(double %d) {
entry.0:
  call void @js_gc_entry_safepoint()
  %a = call i64 @js_object_alloc(i32 0, i32 0)
  %r = call double @rec_polled(double %d)
  ret double %r
}

define double @rec_bare(double %d) {
entry.0:
  %a = call i64 @js_object_alloc(i32 0, i32 0)
  %r = call double @rec_bare(double %d)
  ret double %r
}

define double @rec_pure(double %d) {
entry.0:
  %r = call double @rec_pure(double %d)
  ret double %r
}
"""


def self_test():
    with tempfile.TemporaryDirectory() as tmp:
        path = os.path.join(tmp, "self.ll")
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(SELF_TEST_IR)
        loops, sccs, totals = check(dom.parse_file(path))
    assert loops == ["bare_loop: loop at %loop.1"], loops
    assert sccs == ["rec_bare"], sccs
    assert totals["loops"] == 3 and totals["allocating_loops"] == 2, totals
    assert totals["sccs"] == 3 and totals["allocating_sccs"] == 2, totals
    print("gc_poll_coverage_check self-test: ok")
    return 0


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("paths", nargs="*")
    ap.add_argument("--max-uncovered-loops", type=int, default=None)
    ap.add_argument("--max-uncovered-sccs", type=int, default=None)
    ap.add_argument("-v", "--verbose", action="store_true")
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()
    if not args.paths:
        ap.error("no paths")
    return run(args.paths, args.max_uncovered_loops, args.max_uncovered_sccs, args.verbose)


if __name__ == "__main__":
    sys.exit(main())
