// #7154: every argument of a cross-module direct call must survive the
// lowering of the arguments that follow it.
//
// An argument list is evaluated left to right and each finished value sits in
// a bare SSA register while the later ones are lowered. `lower_call/
// extern_func.rs`'s `perry_fn_<src>__<name>` path lowered the whole list in a
// plain loop with no protection at all, so `f(A, B, alloc(), userCode(), …)`
// leaves A and B naming pre-collection addresses the moment an evacuating
// minor lands in argument 3, 4 or 5.
//
// This is the residual #7227 measured and named. In the `sfw-registry`
// reproducer it is `src/lib/api/alerts.ts`'s module init calling
// `defineApiCall(url, method, {…}, Schema.array(), body => JSON.parse(body))`
// across the module boundary, faulting one frame down at
// `perry_fn_src_lib_api_shared_ts__defineApiCall + 428` inside `js_regexp_test`
// with `obj_type=3` (a string). The compiled shape:
//
//     ldp  d9, d10, [x24, #0x18]   ; url + method, loaded from their
//                                  ; `__perry_init_strings_*` handle globals
//     bl   js_object_alloc_class_inline_keys   ; argument 3  -- ALLOCATES
//     bl   perry_fn_…__SocketAlert / zod       ; argument 4  -- USER CODE
//     bl   js_closure_alloc_singleton          ; argument 5  -- ALLOCATES
//     fmov d0, d9                  ; STALE
//     fmov d1, d10                 ; STALE
//     bl   perry_fn_src_lib_api_shared_ts__defineApiCall
//
// Measured at the fault, which is what makes the diagnosis a fact rather than
// a reading of the disassembly: the handle global held `0x…76561xxx` — the
// post-move address evacuation wrote back — while the register (and therefore
// the callee's shadow slot) held `0x…74eb5d58`, inside the quarantined
// from-space block the reporter named.
//
// Two protections, both exercised below:
//
//   * a STRING LITERAL argument is `OperandProtection::Reload`. Its handle
//     global is a registered root, so the string is never swept — but an
//     evacuating cycle REWRITES that global, so the fix is to emit the load
//     again below the collection point. No runtime call at all.
//   * a LOCAL argument is `OperandProtection::Root`. Re-deriving it would
//     observe an assignment made after the call-time value was taken, so it
//     takes a real temp-root slot instead.
//
// Why the static checker reports nothing here: `gc_root_dominance_check.py`
// classifies a heap-value SOURCE as an `ALLOC_RE` call or a shadow-slot load.
// A load of a string-literal handle global is neither, so the register it
// defines is never tracked as a heap value and no stale use is attributed to
// it. That is the same shape of blind spot `js_implicit_this_set` (#7226) and
// `js_regexp_new` (#7227) each cost a round for.
//
// LIVE BY CONSTRUCTION. `churn` keeps allocating AFTER the back-edge poll that
// collects, so the abandoned from-space bytes are recycled before the callee
// reads them — a stale read returns wrong text instead of the right answer out
// of memory nobody has reused yet. Both arms compare against strings built
// from values re-read after the call, so a stale argument is observable.
//
// The literal arm needs the collection EARLY: a string literal is allocated by
// `__perry_init_strings_*` at startup, so it is young for the first couple of
// minors and tenured after that, and only a young object is evacuated. Under
// `PERRY_GC_ZEAL=1` the first back-edge poll inside `churn` already runs an
// evacuating minor, so iteration 0 is where the literal arm bites. The loop is
// short on purpose — zeal collects at every safepoint.

import { joinArgs } from "./fixtures/gc_call_arg_rooting_pkg/callee.ts";

// Allocates hard, and keeps allocating after the poll that collects, so the
// retired bytes are reused rather than left intact.
function churn(n: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 200; i++) {
    bits.push({ i: i, s: "y" + i, pad: [i, i + 1, i + 2] });
  }
  return bits.length === 200 ? n : -1;
}

function freshUrl(i: number): string {
  return "/v0/orgs/" + i + "/full-scans/[full_scan_id]";
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 8; r++) {
    // Reload arm: BOTH string operands are literals, so both are loads of a
    // `__perry_init_strings_*` handle global — the registry's exact shape.
    const litOut = joinArgs(
      "/v0/orgs/[org_slug]/full-scans",
      "GET",
      { n: churn(r) },
      churn(r),
      churn(r),
    );
    if (litOut !== "/v0/orgs/[org_slug]/full-scans GET " + r + " " + r + " " + r) {
      bad++;
    }
    // Root arm: argument 1 is a local holding a freshly-allocated string
    // (always young, so it moves on every evacuating minor), argument 2 is a
    // literal. One call, both protections.
    const url = freshUrl(r);
    const freshOut = joinArgs(url, "POST", { n: churn(r) }, churn(r), churn(r));
    if (freshOut !== url + " POST " + r + " " + r + " " + r) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
