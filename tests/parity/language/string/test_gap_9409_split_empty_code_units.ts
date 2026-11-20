// Gap test for #9409. `String.prototype.split("")` splits into UTF-16 CODE
// UNITS (§22.1.3.23 → SplitMatch over the code-unit sequence), not into
// Unicode code points: an astral character contributes TWO one-unit strings,
// each a lone surrogate. Perry stores strings as WTF-8 and used to step the
// payload one WTF-8 sequence at a time, so a 4-byte astral sequence produced
// ONE element where Node produces two.
//
// The code-point iterators are deliberately included as controls — `for…of`,
// the spread form and `Array.from` all iterate CODE POINTS and must keep
// returning one element for an astral character. `split("")` is the odd one
// out, and a fix that unified them would be just as wrong in the other
// direction.
//
// This file is byte-compared with `node --experimental-strip-types` by the gap
// suite.

function show(label: string, value: unknown): void {
  console.log(label + ":" + JSON.stringify(value));
}

// Render an array of strings as their code-unit sequences, so lone surrogates
// survive JSON.stringify unambiguously.
function units(parts: string[]): number[][] {
  return parts.map((p) => Array.from({ length: p.length }, (_, i) => p.charCodeAt(i)));
}

const samples: Array<[string, string]> = [
  ["ascii", "abc"],
  ["empty", ""],
  ["latin1", "café"],
  ["bmp", "Ωμέγα"],
  ["cjk", "漢字"],
  ["astral", "😀"],
  ["astral-pair", "😀😀"],
  ["mixed", "a😀b"],
  ["mixed-bmp", "é😀漢"],
  ["flag", "🇩🇪"],
  ["zwj", "👨‍👩‍👦"],
  ["combining", "e\u0301"],
  ["lone-high", "\ud83d"],
  ["lone-low", "\ude00"],
  ["lone-around", "a\ud83db"],
  ["reversed-pair", "\ude00\ud83d"],
];

for (const [name, s] of samples) {
  const parts = s.split("");
  const wellFormed = s.isWellFormed();
  show("len-" + name, s.length);
  show("split-count-" + name, parts.length);
  show("split-units-" + name, units(parts));
  show("split-roundtrip-" + name, parts.join("") === s);
  // Code-point iterators are NOT code-unit based; these must not move.
  show("spread-count-" + name, [...s].length);
  // `Array.from` is skipped for the lone-surrogate samples: it takes a
  // different lowering from the spread/for-of forms above and returns an EMPTY
  // array for any string whose payload is not valid UTF-8. That is a separate
  // defect (`js_array_from_string_codepoints` bails on `str::from_utf8`), not
  // a code-unit question — the spread and for-of rows next to it are the
  // code-point controls this fixture actually needs.
  if (wellFormed) show("from-count-" + name, Array.from(s).length);
  show("forof-count-" + name, (() => { let n = 0; for (const _c of s) n++; return n; })());
  // `charAt` is the code-unit reference the split must agree with.
  show("charat-" + name, Array.from({ length: s.length }, (_, i) => s.charAt(i).charCodeAt(0)));
}

// The `limit` argument counts code units too, and truncation may cut a pair.
show("limit-0", "😀".split("", 0));
show("limit-1", units("😀".split("", 1)));
show("limit-2", units("😀".split("", 2)));
show("limit-3", units("a😀b".split("", 3)));
show("limit-large", units("😀".split("", 99)));

// A dynamic (non-literal) separator takes a different lowering than the
// literal `""` above; both must agree.
const sep = "".concat("");
show("dyn-count", "a😀b".split(sep).length);
show("dyn-units", units("a😀b".split(sep)));
show("dyn-var-count", ((sepVar: string) => "😀".split(sepVar).length)(""));

// An empty RegExp separator is a DIFFERENT operation (RegExpExec-driven): it
// runs through the regex engine, which matches Unicode SCALARS, so it still
// reports 3 parts for "a<astral>b" where Node reports 4. That is the regex
// half of #9409 and needs a code-unit matching path, not a `split` change.

// The scalar-replacement fast paths: when codegen proves the result array does
// not escape and only a constant index is read, `split("")[k]` and
// `split("")[k].length` are answered without building the array at all. They
// have to land on the same code units the array does.
show("scalar-part-0", "a\u{1F600}b".split("")[0].charCodeAt(0));
show("scalar-part-1", "a\u{1F600}b".split("")[1].charCodeAt(0));
show("scalar-part-2", "a\u{1F600}b".split("")[2].charCodeAt(0));
show("scalar-part-3", "a\u{1F600}b".split("")[3].charCodeAt(0));
show("scalar-part-oob", "a\u{1F600}b".split("")[4]);
show("scalar-len-0", "a\u{1F600}b".split("")[0].length);
show("scalar-len-1", "a\u{1F600}b".split("")[1].length);
show("scalar-len-2", "a\u{1F600}b".split("")[2].length);
show("scalar-len-3", "a\u{1F600}b".split("")[3].length);
show("scalar-wellformed-1", "a\u{1F600}b".split("")[1].isWellFormed());

// The well-formedness flag must survive: each half of a split pair is a lone
// surrogate, so it is not well-formed on its own.
const halves = "😀".split("");
show("wellformed-source", "😀".isWellFormed());
show("wellformed-parts", halves.map((h) => h.isWellFormed()));
show("wellformed-rejoined", halves.join("").isWellFormed());
show("tojson-rejoined", JSON.stringify(halves.join("")) === JSON.stringify("😀"));
