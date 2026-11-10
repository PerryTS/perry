// Gap test for #7426: a raw non-ASCII character in a REGEX LITERAL's source
// text was decoded as Latin-1, so `/€/` never matched anything.
//
// The parser's identifier-escape pre-pass stepped through regex literals one
// BYTE at a time and widened each byte with `as char`, so the UTF-8 bytes of
// `€` (E2 82 AC) reached SWC as the three codepoints `â` `\u{82}` `¬`. Nothing
// errored — the pattern simply stopped matching. `\u`/`\x` escapes and
// `new RegExp("€")` were always correct, which is what isolated it to the
// literal path.

const s = "Preis 5 € für Maß ok";

// --- .source must be the source text, not its UTF-8 bytes ----------------
const re = /a€b/;
console.log(re.source);
console.log(re.source.length);
console.log(re.test("a€b"));

// --- raw non-ASCII in a literal matches ----------------------------------
console.log(/€/.test(s));
console.log(/ö/.test("schön"));
console.log(/ß/.test("Maß"));
console.log(/ü/.test("für"));
console.log(/[€]/.test(s));
console.log(/€/u.test(s));
console.log(/[ö-ü]+/.test("für"));
console.log(/schön/i.test("SCHÖN"));

// --- every string-side API that takes a literal --------------------------
console.log("5 € x".replace(/€/, "EUR"));
console.log("5 € x".split(/€/).join("|"));
const m = "5 € x".match(/€/);
console.log(m === null, m ? m[0] : "", m ? m.index : -1);
console.log("a€b€c".replace(/€/g, "-"));

// A realistic German-invoice row: the shape from the bug report.
const ROW = /^\s*(\d+)\s+(.+?)\s+([\d.,]+)\s*€$/;
const row = "  3  Amboßhörnchen groß  12,50 €".match(ROW);
console.log(row === null);
console.log(row ? row[1] : "", row ? row[2] : "", row ? row[3] : "");

// --- controls: these were already correct and must stay correct ----------
console.log(/\u20AC/.test(s)); // escape, not a raw byte
console.log(/\xe9/.test("é"));
console.log(/ok/.test(s)); // all-ASCII literal
console.log(new RegExp("€").test(s)); // constructor path
console.log(new RegExp("a€b").source);
console.log(/^(\d+)\s+(.+?)\s+(\d{3,9})/.test("12 Amboßhörnchen 34567"));

// Escapes must still be escapes after the pre-pass, and a `/` inside a
// character class must still not terminate the pattern.
console.log(/[/€]/.test("€"));
console.log(/ö/.test("ö"));
console.log(/a\/b€/.test("a/b€"));

// Non-ASCII is preserved through .source round-tripping into the constructor.
const rt = new RegExp(/€ß/.source);
console.log(rt.source, rt.test("x€ßy"));
