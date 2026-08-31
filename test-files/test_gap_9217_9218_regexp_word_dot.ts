// Gap test for #9217 and #9218. Perry delegates matching to Rust's regex
// engines, so its JS-to-Rust translation must pin the places where the two
// grammars deliberately differ:
//
//   * ECMAScript \w is ASCII [A-Za-z0-9_], as are \W and the word-ness used
//     by \b / \B. The one exception is i+u, where Unicode simple case folding
//     also admits U+212A KELVIN SIGN and U+017F LATIN SMALL LETTER LONG S.
//   * a non-dotAll `.` excludes LF, CR, LINE SEPARATOR, and PARAGRAPH
//     SEPARATOR. Rust's default dot excludes only LF.
//
// This file is byte-compared with `node --experimental-strip-types` by the gap
// suite. The empty classes at the end guard #9216's cheap `(?s:.)` / `[a&&b]`
// translations while the nearby dot handling changes.

function show(label: string, re: RegExp, subject: string) {
  const match = re.exec(subject);
  console.log(label + ":" + (match === null ? "null" : JSON.stringify([match[0], match.index])));
}

// ASCII control cases.
show("word-ascii", /^\w+$/, "Az_09");
show("nonword-ascii", /^\W+$/, "-!?");
show("word-class-ascii", /^[\w-]+$/, "Az_09-");

// Accented Latin, Greek, CJK, and emoji are non-word even with u or i.
for (const entry of ["café", "Ωμέγα", "漢字", "😀"]) {
  show("word-nonascii", /^\w+$/, entry);
  show("word-nonascii-u", /^\w+$/u, entry);
  show("word-nonascii-i", /^\w+$/i, entry);
  show("nonword-nonascii", /^\W+$/, entry);
  show("class-word-nonascii", /^[\w-]+$/, entry);
  show("class-nonword-nonascii", /^[\W]+$/, entry);
}

// Negated and mixed classes exercise \w/\W while they are nested in a class.
show("negated-word", /^[^\w]+$/, "Ω");
show("negated-nonword", /^[^\W]+$/, "A_9");
show("mixed-word-i-hit", /^[a\w]+$/i, "Z");
show("mixed-word-i-miss", /^[a\w]+$/i, "Ω");
show("mixed-negated-word-i", /^[^a\w]+$/i, "Ω");
show("mixed-nonword-i", /^[a\W]+$/i, "Ω");
show("mixed-negated-nonword-i", /^[^a\W]+$/i, "Z");
show("mixed-negated-nonword-i-sequence", /^[^a\W]+$/i, "café");

// \b and \B use the same ASCII word predicate. Around a lone Greek letter,
// both sides are non-word; next to ASCII x there is a boundary.
show("boundary-greek", /^\bΩ\b$/, "Ω");
show("nonboundary-greek", /^\BΩ\B$/, "Ω");
show("boundary-ascii-greek", /x\bΩ/, "xΩ");
show("nonboundary-ascii-greek", /x\BΩ/, "xΩ");

// In a class, \b is BACKSPACE. Sloppy-mode \B is the identity escape `B`;
// the /u form is a SyntaxError and is covered by the differential runner.
show("class-backspace", /^[\b]$/, "\b");
show("class-backspace-not-b", /^[\b]$/, "b");
show("class-identity-B", new RegExp("^[\\B]$"), "B");
show("class-identity-B-i", new RegExp("^[\\B]$", "i"), "b");

// Kelvin and long-s join the word set only when BOTH i and u are present.
for (const entry of ["K", "ſ"]) {
  show("fold-word-plain", /^\w$/, entry);
  show("fold-word-i", /^\w$/i, entry);
  show("fold-word-u", /^\w$/u, entry);
  show("fold-word-iu", /^\w$/iu, entry);
  show("fold-nonword-iu", /^\W$/iu, entry);
  show("fold-boundary-iu", /^\b.\b$/iu, entry);
  show("fold-nonboundary-iu", /^\B.\B$/iu, entry);
  show("fold-class-word-iu", /^[\w]$/iu, entry);
  show("fold-class-nonword-iu", /^[\W]$/iu, entry);
}

// Non-dotAll dot excludes every LineTerminator, regardless of i/u/m/g.
const terminators = ["\n", "\r", " ", " "];
for (const entry of terminators) {
  show("dot", /^.$/, entry);
  show("dot-i", /^.$/i, entry);
  show("dot-u", /^.$/u, entry);
  show("dot-m", /^.$/m, entry);
  show("dot-g", /^.$/g, entry);
  show("dot-s", /^.$/s, entry);
  show("dot-isu", /^.$/isu, entry);
}
show("dot-tab", /^.$/, "\t");
show("dot-crlf-repro", /.{2}/g, "\t\r\n");
show("dot-crlf-dotall", /.{2}/gs, "\t\r\n");

// #9216 controls: [^] is any character even without s; [] never matches.
for (const entry of ["x", "\n", "\r", " ", " "]) {
  show("negated-empty", /[^]/i, entry);
  show("empty", /[]/i, entry);
}
// Use `u` for the astral control so this fixture does not conflate #9216 with
// Perry's separate, pre-existing non-u UTF-16 code-unit matching gap.
show("negated-empty-u", /[^]/iu, "😀");
show("empty-u", /[]/iu, "😀");
show("word-complements-any", /[\w\W]/i, "Ω");
show("word-complements-empty", /[^\w\W]/i, "A");
