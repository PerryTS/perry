// JST slice 3: literal-aware concat (`js_string_concat_parts`), the inline
// ToPrimitive/Symbol tests on the operands of a `+` chain, and the literal
// prefix + number form. Every line is compared byte-for-byte with Node.

function attempt(f: () => string): string {
  try {
    return f();
  } catch (e) {
    return "throw:" + (e instanceof TypeError ? "TypeError" : String(e));
  }
}

function cps(s: string): string {
  const out: string[] = [];
  for (let i = 0; i < s.length; i++) out.push(s.charCodeAt(i).toString(16));
  return out.join(",");
}

// ---- numbers: -0, NaN, infinities, large and negative integers, fractions ----
const numbers: number[] = [
  0, -0, 1, -1, 9, 10, 99, 100, 12345, 999999999, 1000000000, 2147483647,
  -2147483648, 4294967295, 4294967296, -4294967295, 1e15, 1e21, 1.5, -0.25,
  1e-7, NaN, Infinity, -Infinity, 123456789012, 0.1 + 0.2,
];
for (const n of numbers) {
  console.log("item-" + n + ";", "k" + n, "<" + n + ">" + "!", `${n}:` + n);
}
let acc = "";
for (let i = 0; i < 50; i++) acc += "r" + (i * 7919 % 1000003) + "-" + (i - 25) + ";";
console.log(acc.length, acc.slice(0, 60), acc.slice(-30));

// ---- Symbol operands throw TypeError, after earlier operands convert ----
const sym = Symbol("s");
const log: string[] = [];
const o = {
  valueOf() {
    log.push("o.valueOf");
    return 1;
  },
  toString() {
    log.push("o.toString");
    return "O";
  },
};
console.log(attempt(() => "a" + (sym as any) + "b"));
console.log(attempt(() => (sym as any) + "a" + "b"));
console.log(attempt(() => "a" + "b" + (sym as any)));
console.log(attempt(() => "a" + (o as any) + (sym as any)), log.join("|"));
log.length = 0;
console.log(attempt(() => "a" + (sym as any) + (o as any)), log.join("|"));
log.length = 0;
// A lying number annotation that holds a Symbol or an object.
function lie(x: number): string {
  return "v=" + x + ";";
}
console.log(attempt(() => lie(sym as any)));
console.log(attempt(() => lie(o as any)), log.join("|"));
log.length = 0;

// ---- ToPrimitive: valueOf/toString/Symbol.toPrimitive, order across 3+ parts ----
const p = {
  [Symbol.toPrimitive](hint: string) {
    log.push("p:" + hint);
    return "P";
  },
};
const q = {
  valueOf() {
    log.push("q.valueOf");
    return {};
  },
  toString() {
    log.push("q.toString");
    return "Q";
  },
};
const r = {
  valueOf() {
    log.push("r.valueOf");
    return 2.5;
  },
};
console.log("x" + (o as any) + "y" + (p as any) + "z" + (q as any) + "w" + (r as any) + "!");
console.log(log.join("|"));
log.length = 0;
function f(tag: string, v: any): any {
  log.push("eval:" + tag);
  return v;
}
console.log("a" + f("1", o) + f("2", p) + "b" + f("3", r) + f("4", 7));
console.log(log.join("|"));
log.length = 0;
console.log(attempt(() => "a" + f("1", o) + f("2", sym) + f("3", p)), log.join("|"));
log.length = 0;
// booleans, null, undefined, bigint, arrays and plain objects as operands
console.log("t=" + (true as any) + (null as any) + (undefined as any) + (10n as any) + ([1, [2, 3]] as any) + ({} as any));

// ---- surrogates: pairs minted across operand boundaries, lone halves kept ----
const hi = String.fromCharCode(0xd83d);
const lo = String.fromCharCode(0xde00);
const pair = "<" + hi + lo + ">";
console.log(pair.length, cps(pair), [...pair].length, pair.codePointAt(1)!.toString(16));
const loneHi = "a" + hi + "b";
const loneLo = "a" + lo + "b";
console.log(loneHi.length, cps(loneHi), loneLo.length, cps(loneLo));
const two = "" + hi + lo + hi + lo;
console.log(two.length, cps(two), [...two].length);
const mixed = "é€" + hi + "😀" + lo + 5 + "ü";
console.log(mixed.length, cps(mixed));
console.log(("x" + hi + lo + "y") === "x😀y", ("x" + hi + "y" + lo) === "x😀y");

// ---- boundaries: SSO sizes, empty operands, many parts ----
for (let i = 0; i < 12; i++) {
  const n = i * 11;
  console.log("k" + n, "ab" + n, "abc" + n, "abcd" + n, "abcde" + n, "" + n + "");
}
const e1 = "";
const e2 = "";
console.log(("a" + e1 + e2 + "b").length, ("" + e1 + e2).length);
// Empty operands (heap and inline) next to numbers and literals.
function blank(): string {
  return process.argv.length > 999 ? "z" : "";
}
const emptyA = blank();
const emptyB = blank() + blank();
console.log("x" + emptyA + 42 + "y", "x" + 42 + emptyB + "y", emptyA + 7 + emptyB + ";", 5 + emptyA + "!" + emptyB);
const s32 = "0" + 1 + "2" + 3 + "4" + 5 + "6" + 7 + "8" + 9 + "a" + 10 + "b" + 11 + "c" + 12 + "d" + 13 + "e" + 14 + "f" + 15 + "g" + 16 + "h" + 17 + "i" + 18 + "j" + 19 + "k" + 20;
console.log(s32, s32.length);

// ---- very long results ----
for (const len of [15, 16, 17, 31, 32, 33, 1000, 100000, 3000000]) {
  const body = "x".repeat(len);
  const s = "item-" + body + ";" + len;
  console.log(len, s.length, s.charCodeAt(5), s.charCodeAt(s.length - 1), s.slice(-8));
}

// ---- results used as keys and compared by content ----
const keys = new Map<string, number>();
for (let i = 0; i < 2000; i++) keys.set("k" + (i % 750), i);
console.log(keys.size, keys.get("k0"), keys.get("k749"), keys.has("k750"));
const seen = new Set<string>();
for (let i = 0; i < 3000; i++) seen.add("id-" + (i % 97) + "-" + (i % 13));
console.log(seen.size);
