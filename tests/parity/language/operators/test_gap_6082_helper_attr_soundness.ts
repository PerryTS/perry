// #6082 negative control: LLVM attribute groups (#2 pure / #3 readonly) on
// verified runtime-helper declarations must not change observable behavior
// at -O3. Exercises the user-visible surface of every annotated helper —
// js_is_truthy, js_nanbox_pointer / js_nanbox_get_pointer, and the
// js_typed_{f64,i32,i1,string}_arg_guard / _to_raw family — plus a control
// that a NON-annotated throwing helper (BigInt + Number mix) still throws
// when its result is unused. Compared byte-for-byte against node.

// --- 1) throwing helper, result UNUSED: DCE must not delete the call. ---
function throwingUnusedResult(): string {
  const big: any = 10n;
  const num: any = 3;
  try {
    // js_add (NOT annotated) throws TypeError on BigInt+Number mix; the
    // result is deliberately unused. willreturn on this call would let
    // -O3 delete it and swallow the exception.
    big + num;
    return "no-throw";
  } catch (e: any) {
    return "caught " + (e instanceof TypeError ? "TypeError" : "other");
  }
}
console.log("unused-throw:", throwingUnusedResult());

// Same shape inside a loop (LICM bait): the throw must fire every iteration.
let throwCount = 0;
for (let i = 0; i < 3; i++) {
  try {
    const b: any = 1n;
    const n: any = i;
    b + n;
  } catch {
    throwCount++;
  }
}
console.log("loop-throws:", throwCount);

// --- 2) js_is_truthy (readonly): dynamic truthiness across the tag ladder,
// with allocations interleaved so hoisting/CSE across GC-capable calls
// would be observable if unsound. ---
const truthyProbes: any[] = [
  "", "x", "hello world, a heap string", 0, -0, NaN, 1, 0n, 10n,
  null, undefined, true, false, {}, [], "😀",
];
let truthyBits = "";
for (const p of truthyProbes) {
  // Allocation between checks (string concat) — a moving-GC trigger point.
  truthyBits += p ? "1" : "0";
}
console.log("truthy-ladder:", truthyBits);

// Loop-invariant dynamic condition (LICM bait for the readonly group):
// the string is checked every iteration while the loop body allocates.
const invariant: any = "steady";
let hoistSum = 0;
const junk: string[] = [];
for (let i = 0; i < 50; i++) {
  if (invariant) hoistSum += 1;
  junk.push("alloc" + i);
  if (junk.length > 10) junk.length = 0;
}
console.log("licm-truthy:", hoistSum);

// Truthiness of a value that CHANGES between identical-looking checks —
// unsound CSE of js_is_truthy(v1)/js_is_truthy(v2) would merge these.
let mut: any = "";
const before = mut ? "T" : "F";
mut = mut + "grew";
const after = mut ? "T" : "F";
console.log("cse-guard:", before + after);

// --- 3) js_nanbox_pointer / js_nanbox_get_pointer (pure): objects moving
// through any-typed unions and back. ---
const objA: any = { tag: "A", n: 1 };
const objB: any = { tag: "B", n: 2 };
let pick: any = objA;
const t1 = pick.tag;
pick = objB;
const t2 = pick.tag;
console.log("boxing:", t1 + t2, objA.n + objB.n);

// --- 4) typed arg guards + to_raw: call typed functions through any-typed
// references with both guard-passing and guard-failing argument types. ---
function addNums(a: number, b: number): number {
  return a + b;
}
function pickFlag(b: boolean): string {
  return b ? "yes" : "no";
}
function shout(s: string): string {
  return s + "!";
}
function countUp(n: number): number {
  let acc = 0;
  for (let i = 0; i < n; i++) acc += i;
  return acc;
}

const fAdd: any = addNums;
const fFlag: any = pickFlag;
const fShout: any = shout;
const fCount: any = countUp;

try {
  // Guard-pass paths (typed clones): number/number, boolean, string, int32.
  console.log("guards-pass:", fAdd(2, 3), fFlag(true), fShout("hi"), fCount(5));
  // Guard-fail paths (generic fallback). Probes are limited to inputs where
  // Perry's fallback agrees with node (the fallback's coercion semantics for
  // e.g. string-into-number-slot are a pre-existing, separately-tracked gap
  // unrelated to #6082): fractional into the int32 slot, and falsy
  // non-booleans into the boolean slot.
  console.log(
    "guards-fail:",
    fCount(2.5),
    fFlag(0 as any),
    fFlag("" as any),
  );
} catch (e: any) {
  console.log("guard-threw:", e instanceof TypeError ? "TypeError" : "other");
}

// Guards inside an allocating loop: same function, alternating guard-pass
// (integral) and guard-fail (fractional) argument classes — CSE/hoist of
// the guard across iterations would misroute dispatch and skew the sums.
let mixed = "";
for (let i = 0; i < 6; i++) {
  const arg: any = i % 2 === 0 ? i : i + 0.5;
  mixed += fCount(arg) + "|";
}
console.log("guard-alternate:", mixed);

console.log("done");
