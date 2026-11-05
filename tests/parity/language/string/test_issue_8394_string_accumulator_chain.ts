// #8394: an n-way concat fold must retain the growing local prefix and fuse
// only the fixed-size suffix. Folding the prefix into js_string_concat_chain
// copies it on every iteration and makes this standard builder pattern O(n²).

function build(n: number): string {
  let seen = "";
  for (let i = 0; i < n; i++) {
    seen = seen + "[" + "abc" + "]";
  }
  return seen;
}

const built = build(2_000);
console.log("built", built.length, built.slice(0, 5), built.slice(-5));

// The existing unique-owner hint must still preserve immutable aliases. The
// first append after aliasing allocates a new accumulator before later growth
// can happen in place.
let current = "prefix";
const alias = current;
current = current + "[" + "next" + "]";
console.log("alias", alias, current);

// Type annotations are erased. This head pair is numeric at runtime and must
// remain opaque: (42 + 8) + "x" is "50x", not "428x".
let lied: string = 42 as any;
lied = lied + (8 as any) + "x";
console.log("lie", lied);
