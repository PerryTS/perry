// Test: repsel Phase 4a.0 — numeric-proven logical selections (`a || b`,
// `a && b`, `a ?? b`) in arithmetic, condition, and element-store contexts
// (#6904 histogram shape). Validated byte-for-byte against
// `node --experimental-strip-types`.
//
// Edges: hole vs explicit-undefined reads through `|| 0` / `?? 0` / `&& x`,
// NaN vs undefined under `??` (the nullish test must see the UNCOERCED
// value), -0 truthiness and identity, short-circuit side effects, string /
// union / bigint operands staying off the numeric fast path, and a
// passed-through `undefined` stored into a `number[]` element (must stay
// `undefined`, never NaN).

// --- histogram shape: (counts[v] || 0) + 1 over a masked index ---
function histogram(data: number[], size: number): number[] {
  const counts: number[] = new Array(size);
  const mask = size - 1;
  for (let i = 0; i < data.length; i++) {
    const v = data[i] & mask;
    counts[v] = (counts[v] || 0) + 1;
  }
  return counts;
}
const data: number[] = [];
// Park-Miller LCG: every intermediate stays below 2^53, so the sequence is
// exact in f64 on any engine (a multiplier that overflows 2^53 would leave
// the values implementation-rounding-sensitive).
let seed = 12345;
for (let i = 0; i < 1000; i++) {
  seed = (seed * 48271) % 2147483647;
  data.push(seed);
}
const h = histogram(data, 16);
console.log(h.join(","));
let total = 0;
for (let i = 0; i < h.length; i++) total += h[i] || 0;
console.log(total);

// --- hole vs explicit undefined through || / ?? / && ---
const holey: number[] = new Array(5);
holey[1] = 0;
holey[2] = NaN;
holey[3] = -0;
console.log(holey[0] || 7); // hole reads undefined -> falsy -> 7
console.log(holey[1] || 7); // 0 -> 7
console.log(holey[2] || 7); // NaN -> 7
console.log(Object.is(holey[3] || 7, 7)); // -0 falsy -> 7
console.log(holey[0] ?? 7); // undefined -> 7
console.log(holey[2] ?? 7); // NaN is NOT nullish -> NaN
console.log(Object.is(holey[3] ?? 7, -0)); // -0 is NOT nullish -> -0
console.log(holey[0] && 7); // undefined && -> undefined
console.log((holey[0] || 0) + 1); // 1
console.log((holey[2] ?? 0) + 1); // NaN
console.log((holey[0] ?? 0) + 1); // 1
if (holey[0] || 0) {
  console.log("truthy");
} else {
  console.log("falsy");
}

// hole-vs-undefined observability must be untouched
console.log(0 in holey, 1 in holey);
console.log(Object.keys(holey).join(","));
console.log(JSON.stringify(holey));

// --- passed-through undefined stored into number[] slots ---
const dst: number[] = new Array(3);
const src: number[] = new Array(3);
src[0] = 5;
dst[0] = src[1] && 9; // undefined && 9 -> undefined must be stored
dst[1] = src[0] && 9; // 5 && 9 -> 9
console.log(dst[0], dst[1]);
console.log(JSON.stringify(dst));
console.log(0 in dst, 2 in dst);

// --- arithmetic results through logical in number stores ---
const out: number[] = [];
out.push((src[0] || 0) * 2);
out[1] = (src[1] || 0) - 1;
out[2] = -(src[0] ?? 0);
console.log(out.join(","));

// --- short-circuit side effects must be preserved ---
let calls = 0;
function eff(): number {
  calls++;
  return 42;
}
const nums: number[] = [3];
console.log(nums[0] || eff()); // 3; eff NOT called
console.log(calls); // 0
console.log(nums[1] || eff()); // undefined -> 42
console.log(calls); // 1
console.log((nums[0] && eff()) + 1); // 43
console.log(calls); // 2
console.log((nums[1] ?? eff()) + 1); // 43
console.log(calls); // 3
console.log((nums[0] ?? eff()) + 1); // 4; eff not called
console.log(calls); // 3

// --- string / union operands keep JS semantics (concat, not fadd) ---
const name: string = "";
console.log(name || 5); // "" falsy -> 5
console.log(5 || name); // 5
const mixed: any = "abc";
console.log(mixed || 0); // "abc"
console.log((src[0] || 0) + "s"); // "5s" string concat
console.log(("a" && 3) + 1); // "a" truthy -> 3 -> 4

function pick(flag: boolean): number | undefined {
  return flag ? 5 : undefined;
}
const u = pick(false);
console.log((u || 0) + 1); // 1

// --- bigint operands must stay off the numeric fast path ---
const b1: bigint = 5n;
const b2: bigint = 0n;
console.log(b1 || 99n);
console.log(b2 || 99n);
console.log((b1 && 3n) * 2n);
console.log(b2 ?? 7n);

// --- -0 identity through || / ?? in number context ---
const negz: number = -0;
console.log(Object.is(negz || 0, 0)); // -0 falsy -> +0 (right side)
console.log(Object.is(negz ?? 1, -0)); // -0 not nullish -> -0
console.log(1 / (negz || Infinity)); // -0 falsy -> Infinity -> 0
