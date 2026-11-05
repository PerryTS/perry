// `s[i]` and the property lookups that share its entry point. The character a
// string index answers is a value, not a cell: a short-string receiver must
// give the same answer as the same text on the heap, and both must agree with
// Node for non-index keys, out-of-range indices, astral pairs and lone
// surrogates.

function at(s: string, i: number): string {
  return s[i];
}
function atKey(s: string, k: any): any {
  return (s as any)[k];
}

// Short (SSO-eligible) and long (heap) receivers with identical text.
const short = "abc";
const long = ["abc", "defghijkl"].join("");
console.log("short", at(short, 0), at(short, 1), at(short, 2));
console.log("long", at(long, 0), at(long, 3), at(long, 11));

// The answer must compare equal and behave as a string wherever it flows.
const c = at(short, 1);
console.log("identity", c === "b", c == "b", typeof c, c.length, c.charCodeAt(0));
console.log("concat", c + c, ("x" + c).length, [c, c].join("-"));
console.log("in-map", new Map([["b", 1]]).get(c), new Set(["b"]).has(c));
console.log("as-key", { b: 7 }[c as "b"], JSON.stringify({ [c]: 1 }));

// Out of range, negative, fractional, and non-index keys.
console.log("oob", at(short, 3), at(short, -1), at(short, 1.5), at(long, 99));
console.log("keys", atKey(short, "0"), atKey(short, "01"), atKey(short, "1.0"), atKey(short, ""));
console.log("length", atKey(short, "length"), atKey(long, "length"));
console.log("proto", typeof atKey(short, "toUpperCase"), atKey(short, "nope"));

// -0 and NaN keys.
console.log("weird", atKey(short, -0), atKey(short, NaN), atKey(short, Infinity));

// Non-ASCII: multi-byte code points make the byte index and the UTF-16 index
// disagree, and an astral character indexes as its two surrogate halves.
const accented = "héllo";
const astral = "a😀b";
console.log("accented", accented[0], accented[1], accented[2], accented.length);
console.log("astral", astral.length, astral[0], astral[3], astral[1] === "\uD83D", astral[2] === "\uDE00");
console.log("astral-codes", astral.charCodeAt(1), astral.charCodeAt(2), astral.codePointAt(1));

// A lone surrogate survives a round trip through the index path.
const lone = "a\uD800b";
console.log("lone", lone.length, lone.charCodeAt(1), lone[1] === "\uD800", (lone[1] + "").length);

// Short receivers whose payload is multi-byte: the packed bytes are not the
// UTF-16 units, so the index must still count code units.
const shortMulti = "é1";
console.log("short-multi", shortMulti.length, shortMulti[0], shortMulti[1], shortMulti[2]);

// Every character of a mixed string, through both entry points.
let walked = "";
for (let i = 0; i < astral.length; i++) walked += astral[i];
console.log("walk", walked === astral, walked.length);

// A String object (not a primitive) keeps object semantics.
const boxed: any = new String("xy");
console.log("boxed", boxed[0], boxed[1], boxed[2], boxed.length, typeof boxed);

// Index reads through a hot loop, the shape the inline path is built for.
let acc = 0;
for (let i = 0; i < 1000; i++) acc += long[i % long.length].charCodeAt(0);
console.log("hot", acc);
