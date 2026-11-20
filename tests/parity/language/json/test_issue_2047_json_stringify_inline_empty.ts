// Issue #2047: `JSON.stringify(f())` returned `undefined` instead of `"[]"`
// when `f` was a "producer" function (matches the deforestation shape — a
// local `let out = []; ...; return out`). The deforest pass rewrote `f` to
// take a trailing accumulator parameter and changed `return out` to
// `return undefined`. Inline call sites in expression position (here,
// `JSON.stringify(f())`) were missed by the unsafe-call-site scan because
// the scan's local `walk_expr_children` skipped over uncommon `Expr`
// variants like `JsonStringifyFull` — so `f` got rewritten but the inline
// call didn't, and the caller saw `undefined`.

function makeStrings(): string[] {
  const out: string[] = [];
  return out;
}

function makeNumbers(): number[] {
  const out: number[] = [];
  return out;
}

// All three branches should print "[]" in Node and Perry. Pre-fix, the
// inline branches printed "undefined".
console.log("inline empty:", JSON.stringify(makeStrings()));
console.log("inline empty (number[]):", JSON.stringify(makeNumbers()));

const assigned = makeStrings();
console.log("assigned empty:", JSON.stringify(assigned));

// The non-empty inline path was already correct because non-empty array
// producers don't match the `let out = []; return out` MVP shape (they
// have push sites). Keep the assertion so a regression in the unrelated
// path doesn't slip through.
function withPush(): string[] {
  const out: string[] = [];
  out.push("a");
  out.push("b");
  return out;
}
console.log("inline non-empty:", JSON.stringify(withPush()));
