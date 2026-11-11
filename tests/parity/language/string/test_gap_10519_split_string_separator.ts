// Gap test for #10519. `str.split(sep, limit)` with a string separator on a
// string receiver answers from a byte split before any of the general
// algorithm's setup runs. That fast entry has to agree with the general path
// on every input it accepts (inline and heap separators, one-byte and
// multi-byte separators, every numeric `limit`, non-ASCII receivers, results
// longer than its 16 inline part slots) and has to hand back every input it
// declines (an empty separator, a lone-surrogate separator, a non-numeric
// `limit`, a non-string separator or receiver) to that path unchanged.
//
// This file is byte-compared with `node --experimental-strip-types` by the gap
// suite.

function show(label: string, value: unknown): void {
  console.log(label + ":" + JSON.stringify(value));
}

// Code-unit view, so lone surrogates survive JSON.stringify unambiguously.
function units(parts: string[]): number[][] {
  return parts.map((p) => Array.from({ length: p.length }, (_, i) => p.charCodeAt(i)));
}

const jwt =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJ1MSIsInJvbGUiOiJhZG1pbiJ9.ndKK8_0z8Qw_OyczXXftx3rfjSEDizNo8Sb5awzd7Fw";

// Typed receiver and separator: the codegen path.
show("jwt", jwt.split("."));
show("jwt[1].length", jwt.split(".")[1].length);
show("jwt limit 1", jwt.split(".", 1));
show("jwt limit 2", jwt.split(".", 2));
show("jwt limit 0", jwt.split(".", 0));
show("short", "a.b.c".split("."));
show("edges", ".a..b.".split("."));
show("only sep", "...".split("."));
show("empty receiver", "".split("."));
show("no match", "abc".split(","));
show("sep longer than receiver", "ab".split("abc"));
show("sep equals receiver", "abc".split("abc"));
show("multi-byte sep", "one, two, three, ".split(", "));
show("overlapping prefix sep", "aaaa".split("aa"));
show("sep is prefix at tail", "abab".split("ab"));
show("heap sep", "x--sep--y--sep--z".split("--sep--"));

// Untyped receiver, separator and limit: the dynamic dispatch path.
const anyRecv: any = jwt;
const anyDot: any = ".";
show("any", anyRecv.split(anyDot));
show("any limit", anyRecv.split(anyDot, 2));

// More parts than the inline part buffer holds.
const many = Array.from({ length: 40 }, (_, i) => "p" + i).join(",");
const manyParts = many.split(",");
show("many length", manyParts.length);
show("many first/last", [manyParts[0], manyParts[16], manyParts[39]]);
show("many limit 17", many.split(",", 17).length);
show("many limit 16", many.split(",", 16).length);
show("csv lines", "h1,h2\nv1,v2\nv3,v4\n".split("\n").map((l) => l.split(",")));

// Every numeric limit form: ToUint32.
const limits: number[] = [-1, 1.9, 2 ** 32 + 2, 2 ** 32, NaN, Infinity, -Infinity, 2 ** 31, 3];
for (const l of limits) {
  show("limit " + String(l), "a,b,c,d".split(",", l));
}

// Non-ASCII receivers and separators.
show("latin1", "café,naïve,über".split(","));
show("latin1 sep", "aébéc".split("é"));
show("astral recv", "😀-😁-😂".split("-"));
show("astral recv lengths", "😀-😁-😂".split("-").map((p) => p.length));
show("astral sep", "a😀b😀c".split("😀"));
show("cjk", "東京・大阪・名古屋".split("・"));

// Lone surrogates: declined by the byte split, answered by UTF-16 units.
const pair = "\u{1F600}";
const low = pair.charAt(1);
const high = pair.charAt(0);
show("low half sep", units((pair + pair).split(low)));
show("high half sep", units(("x" + pair + "y").split(high)));
const loneRecv = "a" + low + "b" + low + "c";
show("lone recv", units(loneRecv.split("b")));
show("lone recv well-formed", loneRecv.split("b").map((p) => p.isWellFormed()));

// Shapes the fast entry declines.
show("empty sep", "abc".split(""));
show("empty sep limit", "abc".split("", 2));
show("undefined sep", "a,b".split(undefined));
show("undefined sep limit 0", "a,b".split(undefined, 0));
const nullSep: any = null;
show("null sep", "anullbnullc".split(nullSep));
const numSep: any = 1;
show("number sep", "a1b1c".split(numSep));
const strLimit: any = "2";
show("string limit", "a,b,c".split(",", strLimit));
const objLimit: any = { valueOf: () => 1 };
show("object limit", "a,b,c".split(",", objLimit));
const objSep: any = { toString: () => "," };
show("object sep", "a,b,c".split(objSep));
const custom: any = { [Symbol.split]: (s: string, l: unknown) => ["custom", s, String(l)] };
show("Symbol.split", "a,b".split(custom, 5));
// Also keeps the regex engine linked under auto-optimize: without it the
// general path has no `@@split` and scans lone-surrogate separators by byte,
// so the lone-surrogate and Symbol.split cases above would differ from Node
// for reasons outside #10519.
show("regex sep", "a1b22c".split(/\d+/));
const numRecv: any = 12321;
show("number receiver", String.prototype.split.call(numRecv, "2"));

// The parts are fresh, independent strings.
const parts = "k=v;k2=v2".split(";");
show("parts", parts.map((p) => p.split("=")));
show("joined", parts.join("|"));
show("indexing", [parts[0].length, parts[1].charAt(1), parts[1] === "k2=v2"]);

// Repeated splits: the result array and its parts must survive collection.
let checksum = 0;
const kept: string[][] = [];
for (let i = 0; i < 20000; i++) {
  const p = (jwt + "." + i).split(".");
  checksum += p[1].length + p[3].length;
  if (i % 5000 === 0) kept.push(p);
}
show("checksum", checksum);
show("kept", kept.map((p) => p[3]));
