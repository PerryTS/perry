// Issue #291: three independent SIGSEGV repros:
// (1) `(maybeArr || []).slice().sort(cmp)` — Logical-typed receiver
//     fell through to js_native_call_method's dispatch tower, which had
//     no "slice" arm for arrays, returned NULL_OBJECT_BYTES, then sort
//     deref'd it as null.
// (2) Class method body invoking an arrow that captures `this`
//     (`ids.map(() => this.value)`) — codegen wrote 0.0 into the
//     reserved `this` capture slot instead of loading from the
//     enclosing method's `this_stack`.
// (3) Same capture-this shape but inlined into a module-level call site
//     (`example.method()` after `const example = new X()`) — the
//     inliner's substitute_this had no Closure arm so `Expr::This`
//     inside the arrow stayed un-rewritten after the surrounding method
//     was hoisted into module init.

// (1) Logical-typed receiver chained through .slice().sort(cmp)
const source: number[] | undefined = undefined;
const values = (source || []).slice().sort((a: number, b: number) => a - b);
console.log("1 slice-sort length:", values.length);

// (1b) Same shape, narrowed through ?? — also Logical at the HIR level
const v2 = (source ?? []).slice().sort((a: number, b: number) => a - b);
console.log("1b nullish-slice-sort length:", v2.length);

// (1c) `any`-typed Call result reached through slice().sort(cmp) —
// runtime-side defensive arm catches this when codegen can't resolve
function makeArr(): any { return [3, 1, 2]; }
const v3 = makeArr().slice().sort((a: number, b: number) => a - b);
console.log("1c any-call slice-sort first:", v3[0], "len:", v3.length);

// (2) ids.map(() => this.value) inside a class method called from a
// module-level local
class Mapper {
  private value = 99;

  getValues(ids: number[]): number[] {
    return ids.map(() => this.value);
  }
}
const m = new Mapper();
console.log("2 map-this length:", m.getValues([1, 2, 3]).length);
console.log("2 map-this first:", m.getValues([1, 2, 3])[0]);

// (3) Generic helper forwarding a `this`-capturing arrow
function run<T>(callback: () => T): T {
  return callback();
}
class Holder {
  private value = 42;
  getValue(): number {
    return run(() => this.value);
  }
}
const h = new Holder();
console.log("3 helper-this:", h.getValue());

// (4) Several variants exercising the capture wiring under stress
class Counter {
  private n = 10;
  doubleAndPlus(extras: number[]): number[] {
    return extras.map((x) => x + this.n);
  }
  withFilter(): number[] {
    return [1, 2, 3, 4, 5].filter(() => this.n > 0);
  }
}
const c = new Counter();
console.log("4a stress-add:", c.doubleAndPlus([1, 2, 3]).join(","));
console.log("4b stress-filter:", c.withFilter().length);

// (5) Class with multiple `this`-capturing arrows. Uses numeric
// arithmetic — string concat inside `this`-capturing closures hits
// a separate pre-existing type-inference gap (PropertyGet through
// `this` doesn't propagate the field's String type, so `+` falls
// into the numeric path). Out of scope for #291.
class MultiArrow {
  private base = 100;
  private bonus = 7;
  combine(extras: number[]): number[] {
    return extras.map((v) => this.base + v + this.bonus);
  }
}
const ma = new MultiArrow();
console.log("5 multi-arrow:", ma.combine([1, 2, 3]).join(","));
