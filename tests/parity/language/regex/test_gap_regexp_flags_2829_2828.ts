// Gap test for #2829 (RegExp flag/pattern validation + canonical .flags) and
// #2828 (sticky/dotAll/unicode/hasIndices flag getters).
//
// Scope note: the Rust `regex` crate cannot implement true sticky (`y`)
// MATCHING or `d` (hasIndices) match.indices, so this test only asserts the
// flag GETTER properties for `y`/`u`/`d`, the canonical `.flags` ordering,
// constructor flag/pattern validation, and `s` (dotAll) match behavior (which
// maps onto the regex crate's `(?s)` mode).

// --- Canonical .flags ordering -------------------------------------------
const re = /x/gimsuy;
console.log(re.flags);
console.log(re.global, re.ignoreCase, re.multiline, re.sticky, re.dotAll, re.unicode);
console.log(re.source);

// Per-flag isolation
console.log(/a/g.flags, /a/g.global, /a/g.sticky);
console.log(/a/y.flags, /a/y.sticky, /a/y.global);
console.log(/a/s.flags, /a/s.dotAll);
console.log(/a/u.flags, /a/u.unicode);
console.log(/a/d.flags, /a/d.hasIndices);
console.log(/a/i.flags, /a/i.ignoreCase);
console.log(/a/m.flags, /a/m.multiline);
console.log(/a/.flags, /a/.global, /a/.sticky, /a/.dotAll, /a/.unicode, /a/.hasIndices);

// new RegExp canonicalizes flag order too
console.log(new RegExp("a", "yusimg").flags);
console.log(new RegExp("a", "mig").flags);

// --- dotAll matching behavior (regex crate (?s)) -------------------------
console.log(/a.b/s.test("a\nb"));
console.log(/a.b/.test("a\nb"));

// --- Constructor validation throws SyntaxError ---------------------------
function expectSyntaxError(fn: () => void): boolean {
  try {
    fn();
    return false;
  } catch (e) {
    return e instanceof SyntaxError;
  }
}

console.log(expectSyntaxError(() => new RegExp("x", "gg")));   // duplicate flag
console.log(expectSyntaxError(() => new RegExp("x", "Q")));    // invalid flag
console.log(expectSyntaxError(() => new RegExp("(", "")));     // invalid pattern
console.log(expectSyntaxError(() => new RegExp("x", "gimg"))); // duplicate among many
console.log(expectSyntaxError(() => RegExp("x", "z")));        // bare call invalid flag
console.log(expectSyntaxError(() => new RegExp("x", "gim")));  // valid → false
