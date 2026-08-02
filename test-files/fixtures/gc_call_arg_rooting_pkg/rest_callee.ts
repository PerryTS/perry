// #7154 fixture: the CROSS-MODULE callee with a trailing `...rest` for
// `test-files/test_gap_gc_rest_argument_rooting.ts`.
//
// It lives in its own module for the same reason
// `gc_call_arg_rooting_pkg/callee.ts` does — the defect under test is in
// `lower_call/extern_func.rs`'s cross-module `perry_fn_<src>__<name>` path, and
// a same-file callee compiles through `func_ref.rs` instead. It is a SEPARATE
// file from `callee.ts` because the arm is chosen by the callee's signature:
// `joinArgs` has no rest param and takes the arm #7240 fixed, `joinRest` has one
// and takes the arm this test pins.
//
// The declared signature matters. Two fixed params plus a rest means
// `declared_count == 3` and `fixed_count == 2`, so `url` and `method` are
// lowered into bare registers and then held across the whole rest-array
// construction — `js_array_alloc` plus one `js_array_push_f64` per trailing
// argument, with each trailing argument's own expression lowered in between.
//
// The body reads the fixed params AND the rest contents, so a caller that
// handed over a pre-collection address produces wrong text rather than a latent
// bad pointer.
export function joinRest(
  url: string,
  method: string,
  ...tags: number[]
): string {
  let total = 0;
  for (let i = 0; i < tags.length; i++) {
    total += tags[i];
  }
  return url + " " + method + " " + tags.length + " " + total;
}
