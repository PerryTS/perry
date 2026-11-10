// P3 B3 controls (DESIGN-p3 rev 3.1 §3 B3): the codegen paths that used to
// test for an INT32-tagged Number, driven with operands the facts do not
// prove (values through `any`, JSON.parse, mixed arrays). A Number has one
// encoding, so each path is one numeric test; every non-Number must still
// take its own arm (the negative controls). Output must match node exactly,
// in normal and in check mode.

function vals(): any[] {
  // Unproven on purpose: parsed at run time, mixed kinds.
  const parsed = JSON.parse('[0, 1, 7, -1, 2.5, 2147483647, -2147483648, 4294967296, 1e300, "7", "1", "", null, true, false]');
  return parsed.concat([-0, NaN, Infinity, -Infinity, undefined, 7n, { valueOf() { return 7; } }, [7], "abc", Symbol.iterator]);
}
function show(v: any): string {
  if (typeof v === "symbol") return "sym";
  if (typeof v === "bigint") return v + "n";
  if (typeof v === "object" && v !== null) return Array.isArray(v) ? "[" + v + "]" : "{obj}";
  return Object.is(v, -0) ? "-0" : String(v);
}

// 1. `===` / `!==` of an unproven value against a Number: an int literal, a
// fraction, a Number local, both operand orders.
function eqLit(v: any): string {
  return [v === 7, 7 === v, v !== 7, v === 0, v === 2.5, 2.5 !== v, v === -1, v === 2147483647, v === 4294967296].map(b => b ? 1 : 0).join("");
}
function eqNum(v: any, n: number): boolean { return v === n; }
function neNum(v: any, n: number): boolean { return n !== v; }
// 2. Relational against a Number: undefined, null and booleans convert
// inline; strings, BigInts and objects keep the coercing helper; a Symbol throws.
function rel(v: any, n: number): string {
  try {
    return [v < n, v <= n, v > n, v >= n, n < v, n <= v, n > v, n >= v].map(b => b ? 1 : 0).join("");
  } catch (e) {
    return "throws:" + (e as Error).constructor.name;
  }
}
// 3. Two unproven operands: the `===` prefix and the dynamic relational.
function eqAny(a: any, b: any): boolean { return a === b; }
function ltAny(a: any, b: any): string {
  try { return String(a < b); } catch (e) { return "T"; }
}
// 4. A comparison graph over one unproven local.
function graph(v: any): string {
  const b = v === 0 || v === 7 || v === 2.5 || v !== v;
  let a = "T", c = "T";
  try { a = v >= 1 && v < 8 || v === -1 ? "1" : "0"; } catch (e) {}
  try { c = String(v > 100 || v <= -100 || v >= 2 && v <= 3); } catch (e) {}
  return a + (b ? 1 : 0) + c;
}
// 5. switch: a dense numeric tree, and a tower mixing literal kinds.
function dense(v: any): string {
  switch (v) {
    case 0: return "zero";
    case 1: return "one";
    case 2: return "two";
    case 3: return "three";
    case 7: return "seven";
    case -1: return "minus";
    case 2147483647: return "max";
    case -2147483648: return "min";
    default: return "none";
  }
}
function tower(v: any): string {
  switch (v) {
    case "7": return "str7";
    case 7: return "num7";
    case true: return "true";
    case null: return "null";
    case undefined: return "undef";
    case 0: return "num0";
    case "": return "empty";
    default: return "none";
  }
}
// 6. An unproven array key: only a non-negative int32 Number indexes.
const table = [10, 11, 12, 13, 14, 15, 16, 17];
function at(k: any): string {
  const t: any = table;
  let r: any;
  try { r = t[k]; } catch (e) { return "T"; }
  return typeof r === "function" ? "fn" : String(r);
}
function put(k: any): string {
  const t: any = [0, 0, 0];
  // A huge index would only measure sparse-array growth.
  if (typeof k === "number" && k > 1000) return "big";
  try { t[k] = 5; } catch (e) { return "T"; }
  return t.length + ":" + t[0] + t[1] + t[2];
}
// 7. Typed entries: Number and int32 lanes, called with every kind.
function addNum(a: number, b: number): number { return a + b; }
function mixI32(a: number, b: number): number { return (a ^ b) | (a & 255); }
function isPos(a: number): boolean { return a > 0; }
class Bits {
  seed: number;
  constructor(seed: number) { this.seed = seed; }
  mix(a: number, b: number): number { return ((a ^ b) + this.seed) | 0; }
  scale(a: number): number { return a * 1.5 + this.seed; }
  get(i: number): number { return table[i] ?? -1; }
}
const bits = new Bits(3);
function callEntries(v: any): string {
  const out: string[] = [];
  const f: any = addNum, g: any = mixI32, h: any = isPos, b: any = bits;
  for (const call of [() => f(v, 1), () => f(1, v), () => g(v, 5), () => g(5, v), () => h(v), () => b.mix(v, 9), () => b.scale(v), () => b.get(v)]) {
    try { out.push(show(call())); } catch (e) { out.push("T"); }
  }
  return out.join(" ");
}
// 8. Closures: a direct typed call and a captured Number.
function closures(v: any): string {
  const k = 2;
  const mul = (x: number): number => x * k;
  const add = (x: number, y: number): number => x + y + k;
  const m: any = mul, a: any = add;
  let r = "";
  try { r += show(m(v)) + " " + show(a(v, 1)) + " " + show(a(1, v)); } catch (e) { r += "T"; }
  return r;
}
// 9. A method on an unproven receiver that may be a Number.
function method(v: any): string {
  try { return String(v.toString()) + "/" + typeof v.valueOf(); } catch (e) { return "T"; }
}
// 10. Arithmetic over unproven leaves (the guarded diamond's dynamic arm).
function arith(v: any): string {
  try { return [show(v + 1), show(v - 1), show(v * 2), show(v | 0), show(v >>> 0), show((v >> 1) & 7)].join(","); } catch (e) { return "T"; }
}

const all = vals();
for (let i = 0; i < all.length; i++) {
  const v = all[i];
  console.log(
    show(v) + " | " + eqLit(v) + " " + (eqNum(v, 7) ? 1 : 0) + (neNum(v, 7) ? 1 : 0) + (eqNum(v, NaN) ? 1 : 0) + (eqNum(v, -0) ? 1 : 0)
    + " | " + rel(v, 7) + " " + rel(v, 0) + " " + rel(v, NaN)
    + " | " + graph(v) + " | " + dense(v) + " " + tower(v)
    + " | " + at(v) + " " + put(v) + " | " + callEntries(v) + " | " + closures(v) + " | " + method(v) + " | " + arith(v),
  );
}
// Pairs of unproven operands, every kind against every kind.
let grid = "";
for (let i = 0; i < all.length; i++) {
  for (let j = 0; j < all.length; j++) grid += (eqAny(all[i], all[j]) ? "=" : ".") + ltAny(all[i], all[j]).charAt(0);
  grid += "\n";
}
console.log(grid);

// Numbers that only look like the old tagged form: integers computed and
// stored through unproven slots keep comparing as the doubles they are.
const box: any = { n: 5, list: [5, 6, 7] as any[] };
function reread(o: any): string {
  const n = o.n, e = o.list[1];
  return [n === 5, e === 6, n + e, o.list[n - 3], dense(n - 4), dense(e - 6), tower(n + 2), n < e, e <= 6, isPos(n - 5), mixI32(n, e)].join(",");
}
console.log(reread(box), reread(JSON.parse('{"n":5,"list":[5,6,7]}')));
