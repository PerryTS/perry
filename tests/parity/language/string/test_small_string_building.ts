// Small strings built at the site: a string `+` of two short strings, a
// one-digit `String(n)`, a `charAt` operand. Every line must match node; the
// negative controls are the operands the site must hand to the general path.

function squareName(square: number): string {
  return "abcdefgh".charAt(square & 7) + String((square >> 4) + 1);
}
function pair(a: string, b: string): string {
  return a + b;
}
function digit(n: number): string {
  return String(n);
}
function digitTemplate(n: number): string {
  return `${n}`;
}
function digitToString(n: number): string {
  return n.toString();
}
function digitEmptyPlus(n: number): string {
  return "" + n;
}
function charPlus(s: string, i: number, t: string): string {
  return s.charAt(i) + t;
}
function plusChar(t: string, s: string, i: number): string {
  return t + s.charAt(i);
}
function twoChars(s: string, i: number, j: number): string {
  return s.charAt(i) + s.charAt(j);
}
function describe(v: unknown): string {
  return typeof v + ":" + String(v) + ":" + (typeof v === "string" ? v.length : -1);
}
function codes(s: string): string {
  const out: number[] = [];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i));
  return s.length + "[" + out.join(",") + "]";
}

// The shape, over every square.
const names: string[] = [];
for (let sq = 0; sq < 128; sq++) {
  if ((sq & 0x88) === 0) names.push(squareName(sq));
}
console.log(names.join(" "));
console.log(squareName(0x70 | 7), squareName(-1), squareName(1.5), squareName(NaN), squareName(1e10));
console.log(squareName(0x90), squareName(0x7fffffff), squareName(-0x80000000));

// One digit and its neighbours.
for (const n of [0, 1, 5, 9, 10, 11, 99, 100, -1, -9, -10, 1.5, 0.5, 9.5, -0.5, -0, NaN, Infinity, -Infinity, 1e21, 99999, 100000, -9999, -10000, 2 ** 31, 2 ** 53]) {
  console.log(codes(digit(n)), codes(digitTemplate(n)), codes(digitToString(n)), codes(digitEmptyPlus(n)));
}
console.log(Object.is(-0, 0), digit(-0) === "0", digit(9) + digit(0) === "90");

// Pairs: inline + inline at every length split, and past five bytes.
const parts = ["", "a", "ab", "abc", "abcd", "abcde", "abcdef"];
for (const a of parts) {
  const row: string[] = [];
  for (const b of parts) {
    const s = pair(a, b);
    row.push(codes(s));
  }
  console.log(row.join(" "));
}
// Operands built at run time (inline strings), not literals.
const built: string[] = [];
for (let i = 0; i < 7; i++) built.push(String(10 ** i).slice(0, i));
for (const a of built) {
  const row: string[] = [];
  for (const b of built) row.push(pair(a, b));
  console.log(row.join("|"));
}
for (let i = 0; i < 12; i++) {
  const a = String(i);
  const b = String(i * 7);
  const c = pair(a, b);
  console.log(c, c.length, c === a + b, pair(c, a), pair(pair(c, c), c).length);
}

// Non-ASCII and lone surrogates: never joined as bytes.
const hi = "\ud83d";
const lo = "\ude00";
console.log(codes(pair(hi, lo)), pair(hi, lo) === "😀", pair(hi, lo).codePointAt(0));
console.log(codes(pair("é", "x")), codes(pair("x", "é")), codes(pair("é", "é")), codes(pair("€", "a")), codes(pair("a", "€")));
console.log(codes(pair("\u0000", "a")), codes(pair("a", "\u0000")), codes(pair("\u007f", "\u0080")));
console.log(codes(pair(lo, hi)), codes(pair(hi, "a")), codes(pair("a", lo)));
console.log(codes(String.fromCharCode(0xe9) + String.fromCharCode(0x41)), codes(String.fromCharCode(65) + String.fromCharCode(66)));

// A string `+` whose operand is not a string after all (a wrong annotation).
const lie = 7 as unknown as string;
const lie2 = { toString() { return "obj"; } } as unknown as string;
const lie3 = null as unknown as string;
const lie4 = undefined as unknown as string;
const lie5 = 10n as unknown as string;
const lie6 = true as unknown as string;
console.log(describe(pair(lie, lie)), describe(pair(lie, "a")), describe(pair("a", lie)));
console.log(describe(pair(lie2, "a")), describe(pair("a", lie2)), describe(pair(lie3, "a")), describe(pair(lie4, "a")));
console.log(describe(pair(lie5, lie5)), describe(pair(lie6, lie6)), describe(pair(lie6, "x")));
console.log(describe(pair(1.5 as unknown as string, 2.25 as unknown as string)), describe(pair(NaN as unknown as string, "n")));

// `+` with a maybe-number operand.
function maybe(flag: boolean): string | number {
  return flag ? "s" : 4;
}
function addMaybe(flag: boolean, n: number): string | number {
  return (maybe(flag) as any) + n;
}
console.log(describe(addMaybe(true, 1)), describe(addMaybe(false, 1)), describe((maybe(false) as any) + (maybe(false) as any)));

// charAt operands: in range, out of range, non-ASCII receiver, long receiver.
const long = "the quick brown fox jumps over the lazy dog";
const uni = "aé€😀z";
for (const i of [0, 1, 4, 5, 6, 42, 43, -1, 1.7, NaN]) {
  console.log(
    codes(charPlus(long, i, "!")), codes(plusChar("#", long, i)), codes(twoChars(long, i, i + 1)),
    codes(charPlus(uni, i, "!")), codes(plusChar("#", uni, i)), codes(twoChars(uni, i, i + 1)),
    codes(charPlus("ab", i, "cdef")), codes(charPlus("12345", i, "")),
  );
}
console.log(codes(charPlus("x", 0, long)), codes(plusChar(long, "x", 0)).length);
console.log(codes(String(7).charAt(0) + String(8).charAt(0)), codes(String(78).charAt(1) + "abcd"), codes(String(78).charAt(2) + "abcde"));

// The joined value is an ordinary string everywhere else.
const key = squareName(0x34);
const table: Record<string, number> = { e4: 1, d5: 2 };
const m = new Map<string, number>([["e4", 10], ["a1", 11]]);
const set = new Set<string>(["e4"]);
console.log(key, table[key], m.get(key), set.has(key), key === "e4", key < "e5", key > "e3", key.length, key.charCodeAt(1));
console.log(JSON.stringify({ [key]: key }), [key, squareName(0)].sort().join(), key.toUpperCase(), key.padStart(4, "0"), `${key}${key}${key}`);
console.log(key.split("").join("-"), [...key].length, key.at(-1), key.indexOf("4"), key.concat(key), key.repeat(3), typeof key);
switch (squareName(0x11)) {
  case "b2":
    console.log("switch b2");
    break;
  default:
    console.log("switch miss");
}

// A modified prototype is still consulted.
const original = String.prototype.charAt;
String.prototype.charAt = function (this: string, i: number): string {
  return "<" + original.call(this, i) + ">";
};
console.log(squareName(0x34), charPlus("abc", 1, "z"), twoChars("abc", 0, 2));
String.prototype.charAt = original;
console.log(squareName(0x34), charPlus("abc", 1, "z"), twoChars("abc", 0, 2));

// Many results, kept alive across collections.
const kept: string[] = [];
let total = 0;
for (let round = 0; round < 200; round++) {
  for (let sq = 0; sq < 128; sq++) {
    if ((sq & 0x88) !== 0) continue;
    const from = squareName(sq);
    const to = squareName(127 - sq - ((127 - sq) & 0x88 ? 8 : 0));
    const uci = from + to;
    const longer = uci + from + to + String(round);
    total += uci.length + longer.length + uci.charCodeAt(3);
    if ((sq + round) % 97 === 0) kept.push(longer);
  }
}
console.log(total, kept.length, kept[0], kept[kept.length - 1], kept.join("").length);
