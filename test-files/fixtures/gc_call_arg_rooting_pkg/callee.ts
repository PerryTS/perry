// #7154 fixture: the CROSS-MODULE callee for
// `test-files/test_gap_gc_call_argument_rooting.ts`.
//
// It has to live in its own module because the defect under test is in the
// cross-module direct-call lowering (`lower_call/extern_func.rs`'s
// `perry_fn_<src>__<name>` path), which is a different code path from the
// same-module one. A same-file callee would compile through `func_ref.rs` and
// never touch the arm this test pins.
//
// The body reads every string argument, so a caller that handed over a
// pre-collection address produces wrong text rather than a latent bad pointer.
export function joinArgs(
  url: string,
  method: string,
  opts: { n: number },
  schemaTag: number,
  parseTag: number,
): string {
  return url + " " + method + " " + opts.n + " " + schemaTag + " " + parseTag;
}
