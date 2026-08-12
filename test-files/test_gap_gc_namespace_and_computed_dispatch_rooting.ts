// #7210 sections 2 and 3: two flagship "no rooting decision at all" sites.
//
// 1. `codegen/helpers.rs`'s `emit_namespace_populator` stages every exported
//    binding's NaN-boxed value into a plain `[N x double]` stack alloca
//    (`vals_buf`) while the per-entry loop calls allocating helpers
//    (`js_closure_alloc_singleton` for the function/class exports below, a
//    cross-module getter for the re-exported var/function). An
//    already-stored entry had no root of its own, so a later entry's
//    allocation could leave it pointing at from-space by the time
//    `js_create_namespace` read the whole buffer. `import * as ns from
//    "./fixtures/gc_namespace_rooting_pkg/lib"` forces the populator to run
//    for that module (four export kinds: plain var, function, class,
//    re-export) and every entry is read back below.
//
// 2. `lower_call/early_branches.rs`'s `obj[strKey](args)` computed-key
//    method dispatch lowers the receiver, the key and every argument into
//    bare registers in sequence, and (in the static-string-key arm)
//    `unbox_str_handle` — an allocating SSO materialisation — used to run
//    between the args buffer's stores and the consuming call. `dispatch()`
//    below calls a method through a computed string key with two heap
//    arguments, one of which allocates hard enough (`churn`) to reach a
//    moving minor mid-call.
//
// Both are exercised under `PERRY_GC_MOVING_LOOP_POLLS=1` allocation
// pressure and asserted for correctness (not just "doesn't crash") against
// Node's output.

import * as ns from "./fixtures/gc_namespace_rooting_pkg/lib.ts";

function churn(seed: number): number {
  const bits: unknown[] = [];
  for (let i = 0; i < 500; i++) {
    bits.push({ i: i, s: "y" + i });
  }
  return seed + bits.length - 500;
}

class Dispatcher {
  add(a: { v: number }, b: { v: number }): number {
    return a.v + b.v;
  }
}

function dispatch(d: Dispatcher, key: string, n: number): number {
  const a = { v: churn(n) };
  const b = { v: n + 1 };
  return (d as unknown as Record<string, (x: unknown, y: unknown) => number>)[key](a, b);
}

function main(): void {
  let bad = 0;

  // Site 1: every export of `lib.ts`, read back after the whole namespace
  // object (all four entries) has been populated.
  if (ns.tag !== "lib") bad++;
  if (ns.double(21) !== 42) bad++;
  const box = new ns.Box(7);
  if (box.value !== 7) bad++;
  if (ns.CHURN_TAG !== "churn") bad++;
  if (ns.churnFromOther(10) !== 10) bad++;

  // Site 2: computed-key method dispatch with heap args, one of which
  // allocates hard enough to pressure a moving minor.
  const d = new Dispatcher();
  for (let r = 0; r < 300; r++) {
    const got = dispatch(d, "add", r);
    if (got !== r + (r + 1)) bad++;
  }

  console.log(bad);
}

main();
