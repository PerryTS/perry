// #7154: the RECEIVER of `re.test(s)` / `re.exec(s)` must be rooted across the
// `ToString(s)` coercion the lowering emits below it.
//
// `Expr::RegExpTest` / `Expr::RegExpExec` lowered the receiver first, unboxed it
// to a raw `RegExpHeader*` in a bare SSA register, and only THEN emitted
// `js_jsvalue_to_string_coerce`. That coerce is not a bystander: it allocates,
// and on an object argument it dispatches a user `toString`, which is arbitrary
// JS with its own loop back-edge polls. Under `PERRY_GC_MOVING_LOOP_POLLS=1` one
// of those polls runs an evacuating minor while the regexp is live only in that
// register, and `js_regexp_test` then dereferences abandoned from-space memory.
//
// This is the residual #7226 measured and named rather than fixed. In the
// `sfw-registry` reproducer it is `src/lib/api/shared.ts:67`,
// `/\[[a-zA-Z]+\]/.test(url)`, faulting at
// `perry_fn_src_lib_api_shared_ts__defineApiCall + 404`:
//
//     bl   js_regexp_new                 ; ALLOCATES
//     and  x20, x0, #0xffffffffffff      ; raw regexp pointer -> bare register
//     bl   js_jsvalue_to_string_coerce   ; ALLOCATES, runs user toString
//     mov  x0, x20                       ; STALE
//     bl   js_regexp_test                ; faults here
//
// The static checker could not see it, and that is the other half of the bug:
// `ALLOC_RE` carried an alternative spelled `regexp_alloc\w*` while the call is
// named `js_regexp_new`, so the register had no recognised heap-value source.
// No such symbol as `js_regexp_alloc` has ever existed.
//
// LIVE BY CONSTRUCTION. Both arms use a regex LITERAL receiver, which is the
// registry's shape and the strongest one: `js_regexp_new`'s result is held
// ONLY in the register, so nothing else keeps it alive across the coerce. The
// coercion allocates long enough that the minor runs early inside it and the
// abandoned bytes are then reused by the rest of the coercion's own work. The
// answers are checked against known-correct booleans and match text, so a stale
// read is observable rather than latent. Clean under `PERRY_GEN_GC=0`, so the
// evacuating arms are the ones that bite.

// The loop is what matters, not its trip count: under `PERRY_GC_ZEAL=1` the
// FIRST back-edge poll inside it already runs an evacuating minor, which is
// the collection the receiver has to survive. The count is kept modest on
// purpose — zeal collects at every safepoint, so a 4000-trip churn (what the
// sibling #7154 tests use, where the collection has to arrive on its own
// budget) turns this file into a multi-hour run for no extra coverage.
function churn(tag: string): string {
  const bits: any[] = [];
  for (let i = 0; i < 120; i++) {
    bits.push({ i: i, s: "x", pad: [i, i + 1, i + 2] });
  }
  return bits.length === 120 ? tag : "unreachable";
}

class Coercer {
  tag: string;
  constructor(tag: string) {
    this.tag = tag;
  }
  toString(): string {
    return churn(this.tag);
  }
}

function run(): number {
  let bad = 0;
  for (let r = 0; r < 40; r++) {
    // `.test` — the exact registry site.
    if (!/^tag-[0-9]+$/.test(new Coercer("tag-" + r) as any)) {
      bad++;
    }
    if (/^tag-[0-9]+$/.test(new Coercer("nope-" + r) as any)) {
      bad++;
    }
    // `.exec` — the same lowering with the same defect, fixed with it.
    const m = /^tag-([0-9]+)$/.exec(new Coercer("tag-" + r) as any);
    if (m === null || m[1] !== String(r)) {
      bad++;
    }
  }
  return bad;
}

console.log("bad", run());
