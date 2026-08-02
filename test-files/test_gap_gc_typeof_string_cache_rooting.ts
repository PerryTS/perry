// #7211: the interned `typeof` result strings are GC roots and must be
// registered as such.
//
// `js_value_typeof` (builtins/arithmetic.rs) returns one of eight strings and
// caches each in a thread-local `Cell<*mut StringHeader>` so it is built once
// rather than per call. Those cells held a RAW pointer into the NURSERY and
// nothing else referenced the string, so the first minor collection either
// swept it or evacuated it — and the cache kept naming the abandoned bytes for
// the rest of the process. Every later `typeof x === "…"` then handed
// `js_string_equals` a from-space address.
//
// This is the bug that kept `sfw-registry --help` red under a genuine
// `PERRY_GC_MOVING_LOOP_POLLS=1` build after #7206 and #7214 had closed every
// stale register they could find. It is worth naming the difference, because it
// is why the previous rounds could not find it:
//
//   * a #7154-class stale REGISTER goes bad only if a collection happens to
//     land inside a few-instruction window, so it is timing-dependent and needs
//     a workload plus repetition to surface;
//   * an unregistered CACHE goes bad at the first collection and stays bad, so
//     it fails 10/10 — and it is invisible to
//     `scripts/gc_root_dominance_check.py`, which reads emitted LLVM IR and
//     cannot see a runtime-side table at all.
//
// The from-space reporter named it exactly: `obj_type=3 size=40
// retired_by_minor=#0` — a string, a 32-byte header plus `"string"`, retired by
// the very first collection.
//
// LIVE BY CONSTRUCTION. Every `typeof` here is applied to a value read out of an
// array at a runtime index, so none of it folds at compile time and each call
// really does reach `js_value_typeof` and really does read the cache. The churn
// forces minor collections between the first population of the cache and the
// later reads, which is the whole subject: the FIRST iteration primes the cache
// and the ones after it are the test.

function churn(x: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 400; i++) {
    bits.push({ i: i, s: "x" });
  }
  return x + bits.length - 400;
}

function run(): number {
  let bad = 0;
  const vals: any[] = ["a", 1, true, {}, undefined, run];
  const want: string[] = [
    "string",
    "number",
    "boolean",
    "object",
    "undefined",
    "function",
  ];
  for (let r = 0; r < 500; r++) {
    churn(r);
    for (let k = 0; k < 6; k++) {
      // Reads the cached string and compares it against a literal. A cache
      // entry the collector moved but never rewrote makes this compare read
      // from-space.
      if (typeof vals[k] !== want[k]) {
        bad++;
      }
    }
  }
  return bad;
}

console.log("bad", run());
