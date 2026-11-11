// #7231: the materialized `process.env` object is a GC root and must be
// registered as one.
//
// `js_process_env_impl` (process/env_misc.rs) builds `process.env` once with
// `crate::object::js_object_alloc` — the NURSERY — and stores it in a
// thread-local `Cell<f64>`. That cell is the whole reference graph:
// `process.env` is not a field of the `process` object, it is a
// `js_process_env()` CALL that returns the cache. So before this fix the first
// minor collection swept or evacuated the object and every later
// `process.env.X = v` wrote through a dangling pointer into abandoned memory.
//
// The sibling `PROCESS_FINALIZATION_OBJECT` (process.rs) uses the *same*
// materialize-once-cache idiom and IS rooted
// (`scan_process_finalization_roots_mut`), which is what makes this an
// omission rather than a design.
//
// WHY ENUMERATION IS THE OBSERVABLE. A direct `process.env.KEY` READ lowers to
// `js_getenv`, which asks the OS and is therefore correct whatever state the
// cached object is in. What walks the cached object is ENUMERATION —
// `Object.keys(process.env)`, `for…in`, spread — which is exactly how
// `@next/env` and `dotenv` consume it. Testing the read would be a gate that
// cannot fail.
//
// NOT A STALE REGISTER, which is the diagnostic signature worth internalising
// (#7226): an unrooted register goes bad only if a collection lands in a narrow
// window, so it reproduces intermittently. An unregistered CACHE goes bad at
// collection #0 and stays bad, so it reproduces every time — and no static
// checker can see it, because `scripts/gc_root_dominance_check.py` reads
// emitted LLVM IR and a runtime-side table is not in it.
//
// LIVE BY CONSTRUCTION. The keys are written across the churn loop rather than
// all up front, so the object is mutated on both sides of a collection: the
// early keys test that a swept/moved object still enumerates, and the late ones
// test that the write itself landed in the live object rather than in whatever
// was recycled into its bytes. Keys are namespaced and the output is filtered
// to them, so the expectation does not depend on the machine's environment.

const PREFIX = "PERRY_T7231_";

function churn(x: number): number {
  const bits: any[] = [];
  for (let i = 0; i < 400; i++) {
    bits.push({ i: i, s: "env" });
  }
  return x + bits.length - 400;
}

function run(): number {
  let bad = 0;

  // Prime the cache before any collection, then keep writing through it.
  process.env[PREFIX + "0"] = "v0";

  for (let r = 0; r < 400; r++) {
    churn(r);
    if (r % 100 === 0) {
      process.env[PREFIX + String(r / 100 + 1)] = "v" + String(r / 100 + 1);
    }
    // Enumeration walks the CACHED object. A collection that reclaimed or
    // relocated it without a root makes this read abandoned memory.
    const seen: string[] = [];
    for (const k of Object.keys(process.env)) {
      if (k.indexOf(PREFIX) === 0) {
        seen.push(k);
      }
    }
    // key "0" is written before the loop and one more at every r % 100 === 0,
    // and the write for this iteration has already happened above.
    const want = 2 + ((r / 100) | 0);
    if (seen.length !== want) {
      bad++;
    }
  }

  // Spread is the other consumer (`{ ...process.env }`), and it walks the same
  // object through a different path.
  const copy: any = { ...process.env };
  for (let n = 0; n < 4; n++) {
    if (copy[PREFIX + String(n)] !== "v" + String(n)) {
      bad++;
    }
  }

  return bad;
}

console.log("bad", run());
const finalKeys: string[] = [];
for (const k of Object.keys(process.env)) {
  if (k.indexOf(PREFIX) === 0) {
    finalKeys.push(k);
  }
}
finalKeys.sort();
console.log("keys", finalKeys.join(","));
