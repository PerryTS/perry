// Two-part concatenation of short strings (N7). `a + b` whose operands are at
// most five ASCII bytes in total is built inline from the operands' bits
// (`string::short_pair`); every other pair takes the general concat. This
// fixture drives both sides of that line and the values around it: inline vs
// heap operands, ASCII vs not, the 5/6-byte boundary, numbers, typed and
// untyped `+`, identity, equality and Map keys. Output must equal node's with
// the arm on and with PERRY_STRING_FAST_PATHS=0.

function squareName(square: number): string {
  return "abcdefgh".charAt(square & 7) + String((square >> 4) + 1);
}
function promotionName(piece: number): "q" | "r" | "b" | "n" | null {
  if (piece === 5) return "q";
  if (piece === 4) return "r";
  if (piece === 3) return "b";
  if (piece === 2) return "n";
  return null;
}
function uci(from: number, to: number, piece: number): string {
  return squareName(from) + squareName(to) + (promotionName(piece) ?? "");
}
// Untyped operands: the dynamic `+`.
function addAny(a: any, b: any): any {
  return a + b;
}

const show = (label: string, v: unknown) =>
  console.log(label, typeof v, typeof v === "string" ? JSON.stringify(v) + " len=" + v.length : String(v));

// --- the chess shapes -----------------------------------------------------
const names: string[] = [];
for (let sq = 0; sq < 128; sq++) if ((sq & 0x88) === 0) names.push(squareName(sq));
console.log(names.length, names[0], names[7], names[63], names.join("").length);
console.log(uci(0x14, 0x34, 0), uci(0x64, 0x74, 5), uci(0x60, 0x70, 2), uci(0, 0x77, 9));

// --- the 5 / 6 byte boundary, both orders -----------------------------------
const pieces = ["", "a", "ab", "abc", "abcd", "abcde", "abcdef"];
for (const x of pieces) {
  const row: string[] = [];
  for (const y of pieces) {
    const s = x + y;
    row.push(s.length + ":" + (s === x.concat(y)) + ":" + s.charCodeAt(s.length - 1));
  }
  console.log(x.length, row.join(" "));
}

// --- untyped `+`: strings, numbers, mixed ------------------------------------
show("any s+s", addAny("ab", "cd"));
show("any s+s long", addAny("abc", "def"));
show("any s+n", addAny("a", 1));
show("any n+s", addAny(1, "a"));
show("any n+n", addAny(2, 3));
show("any s+-0", addAny("x", -0));
show("any s+NaN", addAny("x", NaN));
show("any s+1e21", addAny("", 1e21));
show("any s+neg", addAny("n", -7));
show("any s+big int", addAny("", 2 ** 53));
show("any s+frac", addAny("", 0.5));
show("any s+null", addAny("a", null));
show("any s+undef", addAny("a", undefined));
show("any s+true", addAny("a", true));
show("any s+obj", addAny("a", { toString() { return "Z"; } }));
show("any s+arr", addAny("a", [1, 2]));

// --- numbers next to short strings (typed) ----------------------------------
const nums = [0, -0, 1, 9, 10, 99, 999, 99999, 100000, -1, -99, 1.5, NaN, Infinity, -Infinity, 1e21, 2 ** 31, 2 ** 32];
console.log(nums.map((n) => "#" + n).join(","));
console.log(nums.map((n) => n + "!").join(","));
console.log(nums.map((n) => String(n) + "").join(","));

// --- unicode and surrogates: never inline-joined, results must still be right
const uni = ["é", "€", "😀", "\uD83D", "\uDE00", "ß", "\u0000", "\u007F", "\u0080"];
for (const u of uni) {
  const l = "a" + u;
  const r = u + "b";
  console.log(JSON.stringify(l), l.length, JSON.stringify(r), r.length, (u + u).length);
}
const pair = "\uD83D" + "\uDE00";
console.log(pair === "😀", pair.length, pair.codePointAt(0), JSON.stringify(pair));
const lone = "x" + "\uD83D";
console.log(lone.length, lone.isWellFormed(), lone.charCodeAt(1));
const nul = "a" + "\u0000" + "b";
console.log(nul.length, nul.charCodeAt(1), nul === "a\u0000b");

// --- identity, equality, Map keys --------------------------------------------
const k1 = "e" + "2";
const k2 = String.fromCharCode(101) + "2";
const k3 = "e2";
console.log(k1 === k2, k1 === k3, k2 === k3, Object.is(k1, k3), k1 < "e3", k1 > "e1");
const m = new Map<string, number>();
for (let i = 0; i < 64; i++) m.set(squareName(i & 0x77), i);
console.log(m.size, m.get("a1"), m.get("h8"), m.get("e" + "4"), m.has("e" + "9"));
const set = new Set<string>();
for (let i = 0; i < 1000; i++) set.add("k" + (i % 37));
console.log(set.size, set.has("k" + 36), set.has("k37"));
const obj: Record<string, number> = {};
for (let i = 0; i < 10; i++) obj["p" + i] = i;
console.log(Object.keys(obj).join(","), obj["p" + "7"]);
const sw = (s: string) => {
  switch (s) {
    case "e2e4": return 1;
    case "e7e8q": return 2;
    default: return 0;
  }
};
console.log(sw(uci(0x14, 0x34, 0)), sw(uci(0x64, 0x74, 5)), sw("e2" + "e5"));

// --- `+=` in loops: short prefix growing past inline -------------------------
let acc = "";
for (let i = 0; i < 12; i++) {
  acc += String.fromCharCode(97 + (i % 26));
  if (i < 7) console.log(i, JSON.stringify(acc), acc.length);
}
console.log(acc);
let digits = "";
for (let i = 0; i < 2000; i++) digits += i % 10;
console.log(digits.length, digits.slice(0, 12), digits.slice(-5));

// --- concat chains and a very long string ------------------------------------
const chain = "a" + "b" + "c" + "d" + "e" + "f" + "g";
console.log(chain, chain.length);
const chain2 = squareName(0) + "-" + squareName(119) + "-" + 7;
console.log(chain2, chain2.length);
let long = "x";
for (let i = 0; i < 18; i++) long = long + long;
console.log(long.length, (long + "y").length, ("y" + long).slice(0, 3), (long + "").length);

// --- many short joins under allocation pressure --------------------------------
let total = 0;
let last = "";
const keep: string[] = [];
for (let i = 0; i < 200000; i++) {
  const s = squareName(i & 0x77) + (i & 1 ? "q" : "");
  total += s.length;
  if ((i & 1023) === 0) keep.push(s + i);
  last = s;
}
console.log(total, last, keep.length, keep[3], keep[keep.length - 1]);
