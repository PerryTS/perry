// parity-env: PERRY_GC_MOVING_LOOP_POLLS=1 PERRY_GC_ZEAL=1
//
// #7154: a cross-module call to a callee with a trailing `...rest` must root
// its fixed parameters AND its accumulating rest array.
//
// THE `parity-env` LINE IS THE TEST. Measured on the parent (`6aeef5baf`):
// without it the harness compiles and runs in the default configuration, the
// broken compiler prints `bad 0` 10/10, and this file gates nothing. Polls are
// off by default since #7161, so the IR has no back-edge safepoint for a minor
// to land on; and without zeal the only collections are allocation-triggered,
// which take `ManualGcScanGuard::force_full_scan` and make the copying minor
// ineligible — so nothing MOVES and a stale register still names a live
// object. `run_parity_tests.sh` applies `parity-env` to the perry compile AND
// the perry run (lines 955/1001), which is exactly what
// `PERRY_GC_MOVING_LOOP_POLLS` needs: it is read at both. With the line, the
// parent SIGSEGVs 10/10. Node ignores both knobs, so the oracle is unchanged.
//
// #7240 fixed `lower_call/extern_func.rs`'s NON-rest arm and named this one as
// a follow-up it could not ship, because the registry does not exercise it and
// an unmeasured GC edit is exactly what the knob-kill policy exists to stop.
// This is that measurement.
//
// The rest arm has TWO unprotected registers where the non-rest arm had one:
//
//   1. THE FIXED PARAMETERS, exactly as in the non-rest arm — except their
//      window does not close when the last argument is lowered. The rest array
//      is materialized afterwards, and materializing it allocates:
//
//          ; url + method -> bare registers
//          bl   js_array_alloc                ; ALLOCATES
//          bl   perry_fn_…__churn             ; trailing arg 1 -- USER CODE
//          bl   js_array_push_f64             ; ALLOCATES (grow)
//          bl   perry_fn_…__churn             ; trailing arg 2 -- USER CODE
//          bl   js_array_push_f64             ; ALLOCATES
//          …
//          fmov d0, d9                        ; STALE url
//          fmov d1, d10                       ; STALE method
//          bl   perry_fn_…__joinRest
//
//   2. THE ACCUMULATOR, which has no analogue in the non-rest arm and is the
//      more dangerous of the two. `current` is a RAW `*mut ArrayHeader` in a
//      bare SSA register, threaded through the push loop, holding the ONLY
//      reference to every argument pushed so far while the NEXT argument's
//      expression is lowered. Nothing roots it, so a minor landing in that
//      window is free to SWEEP the array — not merely move it.
//
// `temp_root::rooted_array_begin`'s doc has named this exact shape as "the
// shape behind every variadic / spread / rest argument list" since #6951, and
// `console_promise.rs` has used it since. This path never adopted it.
//
// Both protections from #7240 are exercised: a STRING LITERAL fixed parameter
// is `OperandProtection::Reload` (its `__perry_init_strings_*` handle global is
// a registered root that evacuation rewrites, so re-emitting the load below the
// collection point is correct and costs no runtime call), and a LOCAL fixed
// parameter is `OperandProtection::Root` (re-deriving it could observe a later
// assignment, so it takes a real temp-root slot).
//
// LIVE BY CONSTRUCTION, the same way #7240's test is: `churn` keeps allocating
// AFTER the back-edge poll that collects, so the retired from-space bytes are
// recycled before the callee reads them. A stale read therefore returns wrong
// text rather than the right answer out of memory nobody has reused yet.
//
// The literal arm needs the collection EARLY: `__perry_init_strings_*` runs at
// startup, so a literal is young for the first couple of minors and tenured
// after that, and only a young object is evacuated. Under `PERRY_GC_ZEAL=1` the
// first back-edge poll inside `churn` already runs an evacuating minor, so
// iteration 0 is where the literal arm bites.

import { joinRest } from "./fixtures/gc_call_arg_rooting_pkg/rest_callee.ts";

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
    // Reload arm: both fixed parameters are literals — loads of a
    // `__perry_init_strings_*` handle global — and three allocating,
    // poll-running trailing arguments are lowered and pushed after them.
    const litOut = joinRest(
      "/v0/orgs/[org_slug]/full-scans",
      "GET",
      churn(r),
      churn(r),
      churn(r),
    );
    if (litOut !== "/v0/orgs/[org_slug]/full-scans GET 3 " + 3 * r) {
      bad++;
    }
    // Root arm: fixed parameter 1 is a local holding a freshly-allocated string
    // (always young, so it moves on every evacuating minor), fixed parameter 2
    // is a literal. One call, both protections.
    const url = freshUrl(r);
    const freshOut = joinRest(url, "POST", churn(r), churn(r), churn(r));
    if (freshOut !== url + " POST 3 " + 3 * r) {
      bad++;
    }
    // Zero trailing arguments still materializes the array (a rest binding must
    // be `[]`), so `js_array_alloc` still sits between the fixed parameters and
    // the call. The cheapest shape that keeps the window open.
    const emptyOut = joinRest(freshUrl(r), "HEAD");
    if (emptyOut !== "/v0/orgs/" + r + "/full-scans/[full_scan_id] HEAD 0 0") {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
