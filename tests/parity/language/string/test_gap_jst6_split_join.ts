// JST slice 4: split pieces built without a per-piece root/reload, join's
// size+copy walk, the string-array `.length` loop region and the
// count-specialized concat entries. Every line is compared with Node.

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

// ---- split: empty strings, separators, unicode, surrogates ----
const sources: string[] = [
  "", "a", ";", ";;", "a;", ";a", "a;b", "a;;b", ";a;b;", "item-1;item-2;item-3;",
  "héllo;wörld;日本語;", "x\u{1F600}y;\u{1F600};z", "lone\uD800;high\uD83D;\uDE00low;",
  "abcdefghijklmnopqrstuvwxyz;0123456789;ABCDEFGHIJKLMNOPQRSTUVWXYZ",
];
const seps: string[] = [";", ";;", "item", "é", "\u{1F600}", "\uD800", "\uDE00", "ab", "z;0"];
for (const s of sources) {
  for (const sep of seps) {
    const parts = s.split(sep);
    console.log(JSON.stringify(s), JSON.stringify(sep), parts.length, parts.map((p) => p.length).join("/"), parts.map(cps).join("|"));
    console.log(cps(parts.join("+")), cps(parts.join("")), cps(parts.join()));
  }
  console.log(s.split(";", 0).length, s.split(";", 1).length, cps(s.split(";", 2).join("|")), s.split(";", 100).length);
}

// ---- very long: enough pieces that the old/young birth and the block
// boundary refusals (collecting fallback) are crossed repeatedly ----
let big = "";
for (let i = 0; i < 20000; i++) big += "p" + i + ";";
const bigParts = big.split(";");
let lenSum = 0;
for (let i = 0; i < bigParts.length; i++) lenSum += bigParts[i].length;
console.log(bigParts.length, lenSum, bigParts[0], bigParts[19999], JSON.stringify(bigParts[20000]));
console.log(bigParts.join(";") === big, bigParts.join("").length);
let wide = "";
for (let i = 0; i < 3000; i++) wide += "é" + i + "\u{1F600}" + (i % 7 === 0 ? "\uD800" : "") + "|";
const wideParts = wide.split("|");
let wideSum = 0;
for (let i = 0; i < wideParts.length; i++) wideSum += wideParts[i].length;
console.log(wideParts.length, wideSum, wideParts.join("|") === wide, cps(wideParts[7]), cps(wideParts[8]));
const longPiece = "q".repeat(100000) + ";" + "r".repeat(70000);
const longParts = longPiece.split(";");
console.log(longParts.length, longParts[0].length, longParts[1].length, longParts.join("--").length);

// ---- join: holey arrays, non-string elements, toString, null/undefined ----
const holey: string[] = new Array(6);
holey[1] = "b";
holey[4] = "e";
console.log(JSON.stringify(holey.join("-")), JSON.stringify(holey.join()), JSON.stringify(holey.join("")));
const mixed: any[] = ["a", 1, -0, 1.5, true, false, null, undefined, "", "é", { toString() { return "obj"; } }, [1, 2], 10n, "\u{1F600}"];
console.log(JSON.stringify(mixed.join("|")), JSON.stringify(mixed.join()), JSON.stringify(mixed.join("")));
console.log(attempt(() => ["a", Symbol("s")].join("-") as string));
const surr: string[] = ["\uD83D", "\uDE00", "x", "\uD83D", "\uDE00"];
console.log(cps(surr.join("")), cps(surr.join("\uDE00")), surr.join("").length);
const shorts: string[] = ["a", "bc", "def", "ghij", "klmno", "pqrstu", "", "", "é", "日"];
console.log(JSON.stringify(shorts.join("")), JSON.stringify(shorts.join(", ")), JSON.stringify(shorts.join("\u{1F600}")));
const lens: string[] = [];
for (let n = 0; n < 40; n++) lens.push("x".repeat(n));
console.log(lens.join("").length, lens.join(",").length, lens.join("ab").slice(-30));
console.log(JSON.stringify([].join("-")), JSON.stringify([""].join("-")), JSON.stringify(["", ""].join("-")));

// ---- string-array `.length` loops: heap, inline, empty, mixed, non-ASCII ----
function totalLength(a: string[]): number {
  let total = 0;
  for (let i = 0; i < a.length; i++) total += a[i].length;
  return total;
}
function totalLengthLocal(n: number): number {
  const arr: string[] = [];
  for (let i = 0; i < n; i++) arr.push("s" + i);
  let total = 0;
  for (let i = 0; i < arr.length; i++) total += arr[i].length;
  return total;
}
console.log(totalLength([]), totalLength([""]), totalLength(["a", "bb", "ccc"]));
console.log(totalLength(["item-123456", "item-1234567", "x".repeat(300)]));
console.log(totalLength(["é", "日本", "\u{1F600}", "a\u{1F600}b", "\uD800", "\uDE00x"]));
console.log(totalLength("a;bb;ccc;;dddd;eeeee;ffffff;".split(";")));
console.log(totalLengthLocal(0), totalLengthLocal(1), totalLengthLocal(10), totalLengthLocal(100000));
const withHole: string[] = new Array(4);
withHole[0] = "ab";
withHole[1] = "cd";
withHole[3] = "ef";
console.log(attempt(() => String(totalLength(withHole))));
const withUndefined: any[] = ["ab", undefined, "cd"];
console.log(attempt(() => String(totalLength(withUndefined))));
const withNull: any[] = ["ab", null];
console.log(attempt(() => String(totalLength(withNull))));
const withObjects: any[] = ["ab", new String("xyz"), { length: 7 }, [1, 2, 3, 4]];
console.log(attempt(() => String(totalLength(withObjects))));
const withNumber: any[] = ["ab", 12345];
console.log(attempt(() => String(totalLength(withNumber))));
const withLateBad: any[] = [];
for (let i = 0; i < 1000; i++) withLateBad.push("v" + i);
withLateBad.push(undefined);
console.log(attempt(() => String(totalLength(withLateBad))));
const proxied: string[] = new Proxy(["a", "bb"], {}) as any;
console.log(attempt(() => String(totalLength(proxied))));
class Sub extends Array<string> {}
const sub = new Sub();
sub.push("a", "bcd");
console.log(totalLength(sub));
const accessor: string[] = ["a", "b", "c"];
Object.defineProperty(accessor, 1, { get() { return "zzzz"; } });
console.log(totalLength(accessor));
const stray: string[] = ["a", "bb"];
(stray as any).foo = 1;
console.log(totalLength(stray));
const shifted: string[] = ["x", "yy", "zzz", "wwww"];
shifted.shift();
console.log(totalLength(shifted), shifted.length);
let pieceSum = 0;
const pieces: string[] = "k1;k22;k333;".split(";");
for (let i = 0; i < pieces.length; i++) pieceSum += pieces[i].length;
console.log(pieceSum, pieces.length);

// ---- concat with 2..6 operands (the count-specialized entries) ----
const objs: any = { toString() { return "O"; }, valueOf() { return 7; } };
const vals: any[] = [0, -0, 1, 99, 100, 12345, 4294967295, 4294967296, 1.5, NaN, -Infinity, "", "é", "\u{1F600}", "\uD800", "\uDE00", "abcdefg", true, null, undefined, objs, 10n];
for (const v of vals) {
  const a = attempt(() => "<" + v + ">");
  const b = attempt(() => "[" + v + "|" + v + "]");
  const c = attempt(() => "a" + v + "b" + v + "c");
  const d = attempt(() => "" + v + "-" + v + "-" + v + "-" + v);
  const e = attempt(() => "k" + v + ":" + v + ":" + v + ":" + v + ":" + v);
  const f = attempt(() => v + "x" + v);
  const g = attempt(() => "1" + "2" + v + "3" + "4" + v + "5" + "6" + v);
  console.log(String(typeof v), cps(a), cps(b), cps(c), cps(d), cps(e), cps(f), cps(g));
}
for (let i = 0; i < 40; i++) {
  const s = "item-" + i + ";" + (i * 31) + "," + (i % 3 === 0 ? "t" : "") + "|" + (i - 20);
  console.log(s, s.length, "[" + s + "]" + i, i + "#" + s);
}
console.log(attempt(() => "a" + Symbol("q") + "b"));
console.log(attempt(() => ("a" + Symbol("q")) as string));
console.log(attempt(() => "a" + "b" + Symbol("q")));
const order: string[] = [];
const p1 = { valueOf() { order.push("p1"); return 1; } };
const p2 = { valueOf() { order.push("p2"); return 2; } };
const p3 = { toString() { order.push("p3"); return "3"; }, valueOf() { order.push("p3v"); return 3; } };
console.log("x" + (p1 as any) + "y" + (p2 as any) + "z" + (p3 as any), order.join(","));
