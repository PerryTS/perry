// The public entry of a function with `number` parameters admits a value into
// the typed clone only when it is a JS Number. Its first tier is an ordered
// (non-NaN) test; every NaN-patterned value takes the second tier, which must
// admit canonical and payload NaNs of either sign and int32 boxes, and send
// every tag (strings, booleans, undefined, null, objects, native handles in
// negative-NaN space) to the generic body. Each witness function is called
// directly with an `any` argument and through a function value.

function selfEq(x: number): number {
  return x === x ? 1 : 0;
}

function inv(x: number): number {
  return 1 / x;
}

function add1(x: number): number {
  return x + 1;
}

function pair(a: number, b: number): number {
  return (a === a ? 10 : 0) + (b === b ? 1 : 0);
}

function sub(a: number, b: number): number {
  return a - b;
}

function show(v: any): string {
  if (typeof v === "number") {
    if (Object.is(v, -0)) return "-0";
    return String(v);
  }
  return typeof v + ":" + String(v);
}

function nanFromBits(hi: number, lo: number): number {
  const bytes = new ArrayBuffer(8);
  const view = new DataView(bytes);
  view.setUint32(0, lo, true);
  view.setUint32(4, hi, true);
  return new Float64Array(bytes)[0];
}

const negZero = -0 * 1;
const values: [string, any][] = [
  ["0", 0],
  ["-0 literal", -0],
  ["-0 computed", negZero],
  ["1.5", 1.5],
  ["-1.5", -1.5],
  ["+Infinity", Infinity],
  ["-Infinity", -Infinity],
  ["MAX_VALUE", Number.MAX_VALUE],
  ["-MIN_VALUE", -Number.MIN_VALUE],
  ["int32 max", 2147483647],
  ["int32 min", -2147483648],
  ["int32 max + 1", 2147483648],
  ["int32 min - 1", -2147483649],
  ["int32 max via |0", (2147483647 | 0)],
  ["int32 min via |0", (-2147483648 | 0)],
  ["uint32 max", 4294967295],
  ["canonical NaN", NaN],
  ["0/0", 0 / 0],
  ["-NaN", -NaN],
  ["sqrt(-1)", Math.sqrt(-1)],
  ["payload +NaN 7ff8", nanFromBits(0x7ff80000, 1)],
  ["payload -NaN fff8", nanFromBits(0xfff80000, 1)],
  ["payload +NaN 7ff9", nanFromBits(0x7ff90000, 0x1234)],
  ["payload -NaN fff9 (handle tag)", nanFromBits(0xfff90000, 0x1234)],
  ["payload -NaN fffc", nanFromBits(0xfffc0000, 7)],
  ["payload +NaN 7fff", nanFromBits(0x7fffffff, 0xffffffff)],
  ["payload -NaN ffff", nanFromBits(0xffffffff, 0xffffffff)],
  ["signalling +NaN 7ff0", nanFromBits(0x7ff00000, 1)],
  ["signalling -NaN fff0", nanFromBits(0xfff00000, 1)],
  ["string", "3"],
  ["empty string", ""],
  ["true", true],
  ["false", false],
  ["undefined", undefined],
  ["null", null],
  ["object", { valueOf() { return 4; } }],
  ["array", [5]],
  ["bigint", 5n],
];

const viaSelfEq: any = selfEq;
const viaInv: any = inv;
const viaAdd1: any = add1;
const viaPair: any = pair;
const viaSub: any = sub;
const direct = selfEq as (x: any) => number;
const directInv = inv as (x: any) => number;
const directAdd1 = add1 as (x: any) => number;

for (const [name, v] of values) {
  let add: string;
  try {
    add = show(directAdd1(v)) + "/" + show(viaAdd1(v));
  } catch (e) {
    add = "throws " + (e as Error).constructor.name;
  }
  let inverse: string;
  try {
    inverse = show(directInv(v)) + "/" + show(viaInv(v));
  } catch (e) {
    inverse = "throws " + (e as Error).constructor.name;
  }
  console.log(name, "selfEq", direct(v), viaSelfEq(v), "inv", inverse, "add1", add);
}

// Native handles (HANDLE_TAG, negative-NaN space) must route to the generic
// body: a typed clone would see a NaN, and `x === x` would be false.
const handles: [string, any][] = [
  ["headers (handle)", new Headers({ a: "b" })],
  ["blob (handle)", new Blob(["x"])],
  ["response (handle)", new Response("r")],
];
for (const [name, h] of handles) {
  console.log(name, "selfEq", direct(h), viaSelfEq(h), "pair", viaPair(h, 1), viaPair(1, h), viaPair(h, h));
}

const pairs: [string, any, any][] = [
  ["num num", 2, 3],
  ["-0 int32", negZero, 7],
  ["NaN num", NaN, 1],
  ["num NaN", 1, NaN],
  ["fff9 payload num", nanFromBits(0xfff90000, 0x1234), 1],
  ["num string", 1, "2"],
  ["int32 min int32 max", -2147483648, 2147483647],
  ["Infinity -Infinity", Infinity, -Infinity],
  ["undefined num", undefined, 1],
];
for (const [name, a, b] of pairs) {
  let diff: string;
  try {
    diff = show(viaSub(a, b));
  } catch (e) {
    diff = "throws " + (e as Error).constructor.name;
  }
  console.log(name, "pair", viaPair(a, b), (pair as any)(a, b), "sub", diff);
}

// A hot loop, so a recursive or repeated typed entry is exercised the same way.
let acc = 0;
for (let i = 0; i < 1000; i++) {
  acc += selfEq(i) + (viaSelfEq(i % 3 === 0 ? NaN : i) as number);
}
console.log("loop", acc);
