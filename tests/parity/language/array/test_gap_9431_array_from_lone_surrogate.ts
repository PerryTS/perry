// #9431 — `Array.from(str)` returned `[]` for any string containing a lone
// surrogate, byte-compared against `node --experimental-strip-types`.
//
// `js_array_from_string_codepoints` ran `std::str::from_utf8` over the payload
// and returned an EMPTY array on `Err`. Perry string payloads are WTF-8, so a
// lone surrogate — a legal payload, produced by slicing a pair or by a chunked
// decoder — made the whole conversion silently yield nothing. Whole-array data
// loss, not a wrong element: the length was 0, not 3.
//
// The spread / `for…of` / `[Symbol.iterator]` forms over the SAME string were
// already correct, which is what made this a wrong answer rather than a
// consistent limitation — so every case asserts all five forms together. Each
// element is reported by CHAR CODE, never printed raw: a lone surrogate has no
// UTF-8 encoding and would reach the terminal as replacement bytes.

function codes(parts: string[]): string {
  return parts
    .map((c) => {
      let out = "";
      for (let i = 0; i < c.length; i++) out += (i ? "+" : "") + c.charCodeAt(i).toString(16);
      return out;
    })
    .join(",");
}

const cases: [string, string][] = [
  ["ascii", "abc"],
  ["lone-high-mid", "a\ud83db"],
  ["lone-low-mid", "a\udc00b"],
  ["lone-high-only", "\ud83d"],
  ["lone-low-only", "\udc00"],
  ["lone-high-start", "\ud83dab"],
  ["lone-high-end", "ab\ud83d"],
  ["pair", "a😀b"],
  ["pair-plus-lones", "x😀y\ud83dz\udc00w"],
  ["two-lone-highs", "\ud83d\ud83d"],
  ["reversed-pair", "\ude00\ud83d"],
  ["empty", ""],
];

for (const [name, s] of cases) {
  const from = Array.from(s);
  console.log("from", name, "src.length", s.length, "len", from.length, codes(from));

  const spread = [...s];
  console.log("spread", name, "len", spread.length, codes(spread));

  const forOf: string[] = [];
  for (const c of s) forOf.push(c);
  console.log("forof", name, "len", forOf.length, codes(forOf));

  // The mapped form takes the same walk, so it was empty too.
  const mapped = Array.from(s, (c: string, i: number) => i + ":" + codes([c]));
  console.log("mapped", name, "len", mapped.length, mapped.join("|"));

  const manual: string[] = [];
  const iter = s[Symbol.iterator]();
  for (let r = iter.next(); !r.done; r = iter.next()) manual.push(r.value as string);
  console.log("iter", name, "len", manual.length, codes(manual));

  // Every form must agree with every other form, element for element.
  console.log(
    "agree",
    name,
    codes(from) === codes(spread) && codes(from) === codes(forOf) && codes(from) === codes(manual),
  );
}

// A lone surrogate carved out of a WTF-8 source keeps its marker: the element
// is a broken half, not a repaired or replaced character.
const half = Array.from("a\ud83db")[1];
console.log("element-length", half.length);
console.log("element-code", half.charCodeAt(0));
console.log("element-well-formed", half.isWellFormed());
console.log("element-json", JSON.stringify(half));
// A whole pair survives as one two-unit element.
const emoji = Array.from("a😀b")[1];
console.log("pair-length", emoji.length);
console.log("pair-codepoint", emoji.codePointAt(0));
console.log("pair-well-formed", emoji.isWellFormed());
// Join round-trips the source.
console.log("rejoin", Array.from("x😀y\ud83dz").join("") === "x😀y\ud83dz");
