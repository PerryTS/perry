// #6697 (follow-up to #6677): a computed-member tagged-template TAG —
// `` String["raw"]`…` `` — threw `TypeError: value is not a function` when the
// tag expression appeared inline inside a nested closure. The dot form
// (`String.raw`…``) took a string-concat fast path that works everywhere, but
// the string-literal computed form fell through to the general tag-desugar,
// whose `String["raw"]` value-read reroute resolved to a non-function once
// emitted inside a function/arrow scope. Minifiers/bundlers routinely rewrite
// `String.raw` to `String["raw"]`, so both forms must match Node in every
// scope. Fix routes both forms through the same fast path.

// --- controls that already worked (regression guards) ---
console.log(String.raw`plain-dot`); // dot, top level
console.log(String["raw"]`plain-computed`); // computed, top level (worked)
console.log((() => String.raw`plain-dot-closure`)()); // dot inside closure
{
  const t = String["raw"];
  console.log((() => t`plain-alias`)()); // aliased-first inside closure
}

// --- the bug: inline computed tag inside a nested closure ---
console.log((() => String["raw"]`plain-BUG`)());

// --- backslash preservation (raw semantics, not cooked) ---
console.log(String["raw"]`a\nb\t\\c`); // top level
console.log((() => String["raw"]`a\nb\t\\c`)()); // inside closure

// --- substitutions preserved through the fast path ---
const x = 42;
const y = "Z";
console.log(String["raw"]`v=${x} and ${y}!`);
console.log((() => String["raw"]`v=${x} and ${y}!`)());
console.log(((): string => String["raw"]`sum=${x + 1}\path`)());

// --- deeper nesting: function declaration + nested arrows ---
function outer(): string {
  const inner = () => String["raw"]`deep\nesting`;
  return inner();
}
console.log(outer());

console.log([1, 2, 3].map((n) => String["raw"]`n\=${n}`).join(","));
