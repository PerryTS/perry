### Bug fixes

**HIR lowering never finished for a class whose own methods each construct a fresh instance of itself while capturing an outer local — the shape every `@noble/curves` elliptic-curve `Point`/arithmetic class uses, which made compiling `ethers` from real source hang indefinitely (#10757).**

```ts
function makeCounter() {
  let base = 10;
  class Cell {
    constructor(v) { this.v = v; }
    get value() { return this.v + base; }   // Cell captures `base`
    plus1() { return new Cell(this.v + 1); } // ...and self-constructs
    plus2() { return new Cell(this.v + 2); }
    // ...
  }
  return new Cell(0);
}
```

#### Root cause

`crates/perry-hir/src/lower/shared_mutable_capture.rs`'s `for_each_nested_capture` was written to find a class genuinely *nested inside* another class's method body (`class Outer { make() { return class Inner { constructor() { n++ } } } }`) by scanning member bodies for capture-forwarding constructions. A self-referential `new Cell(...)` inside `Cell`'s own methods — completely ordinary code for any arithmetic/builder class — matches that same scan, so `for_each_nested_capture` misreads `Cell` as nested inside `Cell`, and `class_mutates_capture` recurses back into the class it started from. Every self-constructing method adds a branch to that self-recursion, at every depth up to the hardcoded `MAX_NESTED_CLASS_DEPTH` (8), recomputing the identical `(class, id)` subproblem from scratch on each branch: exponential in the class's method count, bounded only by the depth cap, so a realistic class (24 methods in `@noble/curves`' `weierstrass.js`) never finished lowering within any practical wait.

Bisecting `weierstrass.js` down to a package-independent synthetic fixture and instrumenting call counts confirmed a **bounded blowup, not a true hang**: calls to `class_mutates_capture` fit `(Bⁿ-1)/(B-1)` for `n=9` almost exactly, at branching factors 1, 2, 3 as the class grows by one method — 252 → 35,770 → 688,870 calls over three additional methods.

#### Fix

Memoize `class_mutates_capture` by `(class_name, id)`, shared across every id `detect_shared_in_body` asks about, with an in-progress set to break cycles instead of the depth cap — the depth cap could in principle also have under-covered a legitimately deep but acyclic nesting chain; the new termination argument is "finitely many distinct `(class, id)` pairs actually reachable," not "bounded to 8 levels." HIR output is byte-identical before/after on every fixture size small enough for the unfixed pass to complete.

Instruction-count differential (`perf stat -e instructions`, lowering only): an 8-method self-constructing class costs about the same either way (2.34B vs 2.21B instructions); a 12-method one costs 11.2× more on the unfixed pass (26.2B) against a flat 2.26B fixed.

With the fix, `ethers@6.17.0`'s full dependency tree (`ethers` + `@noble/curves` + `@noble/hashes` + `@adraffy/ens-normalize` + `aes-js`, 153 modules) lowers and codegens natively in about a minute instead of never finishing. A separate, unrelated link-step failure in that same build (`ethers/src.ts/crypto/crypto.ts`'s `export { createHash, ... } from "crypto"` re-export not resolving against Perry's native `crypto` module) is reported as a follow-up, not fixed here.

Also gives `run_parity_tests.sh`'s compile step a timeout (`PERRY_COMPILE_TIMEOUT`, default 300s) — it previously had none, so a compiler hang/blowup on any one fixture wedged the whole harness instead of failing that fixture; the new gap test relies on this.

Coverage: `test-files/test_gap_10757_self_referential_class_capture.ts` (fails via the harness's new compile timeout on unfixed `main`; passes byte-identical to node with the fix).
