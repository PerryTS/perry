// Gap: class-capture field names must not depend on the working directory
// (#7177).
//
// The per-module salt in `__perry_cap_<id>m<salt>` used to be an FNV hash of
// the CANONICAL ABSOLUTE source path, so the same source compiled from two
// different directories produced different symbol names, different IR, and
// different objects. Measured when it was found: 29 of 29 same-compiler
// control runs differed once the harness used per-run `mkdtemp` working
// directories, and three separate measurement campaigns each had to discover
// the cwd dependency and pin around it.
//
// This test is a BEHAVIOURAL cover, not the reproducibility check — a `.ts`
// gap test cannot compile itself twice from two directories. What it pins is
// the property the salt exists for, which any future re-keying must preserve:
// two modules that share a basename (`a/util.ts` and `b/util.ts`) must get
// DISTINCT salts, so their capture stashes stay isolated; and a subclass
// declared in the same module as its base must keep SHARING the parent stash,
// which requires their salts to be equal.
//
// Both directions matter. A salt that is too coarse (one constant) silently
// merges cross-module stashes; a salt that is too fine (per-class) breaks
// same-module inheritance. The absolute-path salt satisfied both and failed
// reproducibility; the module name satisfies all three.

import { makeA } from "./cap_salt_helper_a.ts";
import { makeB } from "./cap_salt_helper_b.ts";

// Same basename, different directories-worth of identity: distinct captures.
console.log("a:", makeA(10));
console.log("b:", makeB(10));
console.log("a-again:", makeA(1));
console.log("b-again:", makeB(1));

// Same-module inheritance: the subclass must see the parent's captured stash.
function sameModuleChain(seed: number): string {
  const captured = seed * 7;
  class Base {
    base(): number {
      return captured + 1;
    }
  }
  class Sub extends Base {
    sub(): number {
      return this.base() + captured;
    }
  }
  const s = new Sub();
  return s.base() + "/" + s.sub();
}
console.log("chain:", sameModuleChain(3));
console.log("chain2:", sameModuleChain(0));

// A capture reached through two nested classes in one module — same salt, two
// distinct outer ids, so the id half of the name still has to disambiguate.
function twoCaptures(x: number, y: number): string {
  const first = x + 100;
  const second = y + 200;
  class Holder {
    both(): string {
      return first + "," + second;
    }
  }
  return new Holder().both();
}
console.log("two:", twoCaptures(1, 2));
