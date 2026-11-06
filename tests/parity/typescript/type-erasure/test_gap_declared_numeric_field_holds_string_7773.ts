// A declared type is a hint, not a layout fact (CLAUDE.md, Known Limitations:
// annotations are erased, nothing validates them at runtime). Codegen answered
// `is_numeric_expr` = true on the strength of one anyway, and then emitted bare
// f64 arithmetic on whatever the slot actually held.
//
// That is worse than it sounds, because arithmetic on a NaN-BOXED value is not
// a no-op that yields NaN — `fadd`/`fmul` propagate the input NaN's payload, so
// a NaN-boxed string comes back out of the instruction STILL TAGGED AS THAT
// STRING. `typeof (v * 2)` answered "string", and `v + 1` looked as though the
// `+ 1` had simply evaporated.
//
// Three divergences, all silent:
//   #7773 shape 1 — `o.x + 1` gave NaN (the number-context read's cold arm
//                   coerces unconditionally); Node concatenates.
//   #7773 shape 2 — through a refined local (`const v = o.x`) there was no
//                   coerce at all, so the string passed straight through.
//   #7776         — a heterogeneous element stored via `as any`, then summed.
//
// The escape is required in every case: a non-escaping receiver gets
// scalar-replaced, which is a real proof, and prints correctly already.

class C {
  x: number;
  constructor(x: number) {
    this.x = x;
  }
}
function poison(o: C): void {
  (o as any).x = "s";
}

// #7773 shape 1: the add is spec'd to dispatch on the RUNTIME value.
function directAdd(): string {
  const o = new C(1);
  poison(o);
  return `${o.x + 1}`;
}

// #7773 shape 2: through a local whose `number` type codegen INFERRED by
// copying the declared field type. Every operator, because only `+` concatenates
// — the rest are plain ToNumber and must give NaN, not a passed-through string.
function throughRefinedLocal(): string {
  const o = new C(1);
  poison(o);
  const v: any = o.x;
  return `${v + 1} ${v - 1} ${v * 2} ${v / 2} ${typeof (v * 2)}`;
}

// The number on the left: `1 + v` concatenates the other way round.
function numberOnTheLeft(): string {
  const o = new C(1);
  poison(o);
  const v: any = o.x;
  return `${1 + v}`;
}

// #7776: a different-shape element reached through `as any`. The element-shape
// fast clone correctly declines this array at runtime (it is heterogeneous);
// the divergence was in the generic path that then ran.
//
// This one also pins a SECOND bug, found while fixing the first: the
// accumulator. `s` is declared `number` by `let s = 0`, and it really is one —
// until index 4 concatenates and `s` holds a string for the remaining five
// iterations while its static type still says `Number`. A fix that tests only
// the operands whose DECLARED type is suspect skips `s`, `fadd`s a NaN-boxed
// string, passes it through unchanged, and prints `16zw` — the original bug,
// one level up. So the expected value here is load-bearing digit by digit, not
// just "not NaN".
class P {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}
class Q {
  x: string;
  y: string;
  constructor(x: string, y: string) {
    this.x = x;
    this.y = y;
  }
}
function heterogeneousElements(): string {
  const a: P[] = [];
  for (let i = 0; i < 10; i++) a.push(new P(i, i + 1));
  (a as any)[4] = new Q("z", "w");
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    const r = a[i];
    s += r.x + r.y;
  }
  return `${s}`;
}

// An array's declared ELEMENT type is violable the same way a field's is.
function arrayElement(): string {
  const a: number[] = [1, 2, 3];
  (a as any)[1] = "q";
  return `${a[1] + 1} ${a[0] + 1} ${a[1] * 2}`;
}

// An INHERITED field resolves through the same class walk, so it must get the
// same treatment.
class Base {
  n: number;
  constructor(n: number) {
    this.n = n;
  }
}
class Derived extends Base {
  constructor(n: number) {
    super(n);
  }
}
function inheritedField(): string {
  const d = new Derived(3);
  (d as any).n = "j";
  return `${d.n + 1}`;
}

// `a + b + c` is left-associative, and the inner Add is only numeric when both
// of ITS operands are — the recursion in the predicate has to agree with the
// one in `is_numeric_expr` or the chain re-acquires the bad proof one level up.
function chainedAdd(): string {
  const p = new P(1, 2);
  (p as any).y = "Y";
  return `${p.x + p.y + 1}`;
}

// THE OTHER DIRECTION — these must keep answering as numbers. A fix that
// coerced or dispatched everything would pass every assertion above while
// quietly turning ordinary arithmetic into string concatenation, so the honest
// shapes are asserted for VALUE, not merely for "does not crash".
function honestArithmetic(): string {
  const pts: P[] = [];
  for (let i = 0; i < 5; i++) pts.push(new P(i, i * 2));
  let s = 0;
  for (let i = 0; i < pts.length; i++) {
    const q = pts[i];
    s += q.x + q.y;
  }
  const one = new P(3, 4);
  const viaLocal: any = one.x;
  return `${s} ${one.x + one.y} ${viaLocal + 1} ${viaLocal * 2} ${typeof (viaLocal + 1)}`;
}

// A guard failure on an HONEST value must stay on the numeric answer. Adding a
// dynamic property makes the class-field guard fail, so this takes the boxed
// fallback with a value that really is a number — the arm that would break if
// the fix reached for "always concatenate" instead of a runtime tag test.
function addExtra(o: C): void {
  (o as any).extra = 7;
}
function honestGuardFailure(): string {
  const o = new C(5);
  addExtra(o);
  const v: any = o.x;
  return `${typeof v} ${v * 2} ${v + 1}`;
}

// A typed array converts on STORE, so its declared element type is not violable
// and must keep the native read path.
function typedArrayUnaffected(): string {
  const t = new Float64Array(3);
  t[0] = 1.5;
  (t as any)[1] = "5";
  return `${t[0] + 1} ${t[1] + 1}`;
}

console.log("direct add:", directAdd());
console.log("refined local:", throughRefinedLocal());
console.log("number on left:", numberOnTheLeft());
console.log("heterogeneous:", heterogeneousElements());
console.log("array element:", arrayElement());
console.log("inherited field:", inheritedField());
console.log("chained add:", chainedAdd());
console.log("honest arithmetic:", honestArithmetic());
console.log("honest guard failure:", honestGuardFailure());
console.log("typed array:", typedArrayUnaffected());
