// Element kinds (DESIGN-p4l1 §5.4 fixture 10): `-0`, NaN, 2**31, a
// fraction, and non-Numbers stored into an array that looks like int32s,
// read in value and in number context.

function show(a: any[], n: number): string {
  const out: string[] = [];
  for (let i = 0; i < n; i++) {
    const k = i;
    const v = a[k];
    out.push(Object.is(v, -0) ? "-0" : String(v));
  }
  return out.join(",");
}
function sum(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = i;
    s += a[k] * 2;
  }
  return s;
}
function ors(a: number[], n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const k = i;
    s = (s + (a[k] | 0)) | 0;
  }
  return s;
}

const ints = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
console.log("ints", show(ints, 4), sum(ints, 18), ors(ints, 18));

const zero = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
zero[0] = -0;
console.log("-0", show(zero, 3), sum(zero, 18), 1 / zero[0], Object.is(zero[0] * 2, -0));

const nan = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
nan[1] = NaN;
console.log("NaN", show(nan, 3), sum(nan, 18), ors(nan, 18), nan[1] === nan[1]);

const big = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
big[2] = 2 ** 31;
big[3] = -(2 ** 31) - 1;
big[4] = 2 ** 53;
console.log("big", show(big, 5), sum(big, 18), ors(big, 18));

const frac = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
frac[0] = 0.5;
frac[17] = Infinity;
console.log("frac", show(frac, 2), sum(frac, 17), sum(frac, 18), ors(frac, 18));

// Non-Numbers in an array typed as numbers.
const mixed: any[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
mixed[0] = "7";
mixed[1] = null;
mixed[2] = undefined;
mixed[3] = true;
mixed[4] = { valueOf: () => 40 };
mixed[5] = 9n;
console.log("mixed", show(mixed, 4), sum(mixed.slice(0, 5) as number[], 5), ors(mixed as number[], 5));
let threw = "";
try {
  sum(mixed as number[], 6);
} catch (e) {
  threw = (e as Error).constructor.name;
}
console.log("bigint", threw);

// A stored `undefined` is an element; a hole is not.
const und: (number | undefined)[] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18];
und[1] = undefined;
console.log("undefined", show(und, 3), und[1] ?? 5, 1 in und, sum(und as number[], 3));
