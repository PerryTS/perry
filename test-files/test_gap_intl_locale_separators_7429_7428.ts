// Gap: locale-aware digit grouping for `toLocaleString` (#7429, #7428).
//
// Two defects, both byte-visible only against the oracle:
//
// #7429 — the group/decimal separator pair was a single `de`-vs-everything
// branch, written when `de-DE` was the only non-`en` locale under test. Every
// other locale that does not group with `,` was wrong, not only French. The
// French case is the sharp one because the separator is a NARROW no-break
// space (U+202F) for `fr-FR` and a regular no-break space (U+00A0) for
// `fr-CA` — two characters that render identically in a terminal and differ in
// a byte-for-byte diff, which is why this test asserts the code points
// explicitly rather than eyeballing the formatted strings.
//
// #7428 — `bigint.toLocaleString()` with NO arguments produced no grouping at
// all, while `toLocaleString(undefined)` was correct. Codegen lowers the
// zero-arg form to `Expr::DateToLocaleString`, which reaches
// `js_object_default_to_locale_string`; that function had arms for numbers,
// Dates and Temporal values but not for BigInt, so a BigInt fell through to
// Object.prototype's "Invoke(O, 'toString')" tail. The two call forms never
// met, which is exactly why the bug survived: the obvious spelling in a test
// (`toLocaleString(undefined)`) exercises the other path.

const big = 12345678901234567890n;

// #7428: the zero-argument form must group with the default locale, and must
// agree with the explicitly-undefined form.
console.log("zeroarg:" + big.toLocaleString());
console.log("undef:" + big.toLocaleString(undefined));
console.log("small-zeroarg:" + (9876543n).toLocaleString());

// #7429: separators per locale. Printed as code points so the two space
// characters cannot be confused with each other or with a plain ASCII space.
const locales = [
  "en-US",
  "de-DE",
  "fr-FR",
  "fr-CA",
  "es-ES",
  "it-IT",
  "ru-RU",
  "pl-PL",
  "sv-SE",
  "pt-BR",
  "nl-NL",
  "tr-TR",
  "cs-CZ",
  "ja-JP",
];

for (const loc of locales) {
  const s = (9876543210n).toLocaleString(loc);
  const seps = s.replace(/[0-9]/g, "");
  const codes: string[] = [];
  for (let i = 0; i < seps.length; i++) {
    codes.push("U+" + seps.charCodeAt(i).toString(16).toUpperCase().padStart(4, "0"));
  }
  console.log(loc + " " + JSON.stringify(s) + " " + codes.join(","));
}

// The same table through `Intl.NumberFormat`, which shares the resolver, with
// a fractional value so the DECIMAL separator is exercised too — `fr` groups
// with U+202F and separates decimals with a comma, so a locale that got the
// group right and the decimal wrong would still pass the integer-only rows.
for (const loc of ["en-US", "de-DE", "fr-FR", "ru-RU", "nl-NL"]) {
  console.log("nf:" + loc + " " + JSON.stringify(new Intl.NumberFormat(loc).format(1234567.891)));
}
