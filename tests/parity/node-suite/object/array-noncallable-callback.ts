// #4091: array / TypedArray iteration methods must throw a `TypeError` when
// handed a non-callable callback, *before* any iteration. The message matches
// V8 byte-for-byte: `<typeof> <value> is not a function` for Array.prototype
// (and most %TypedArray% methods), with `%TypedArray%.prototype.map` using its
// distinct no-typeof rendering, and `sort` keeping its comparator wording.
function log(label: string, fn: () => unknown) {
  try {
    console.log(label, JSON.stringify(fn()));
  } catch (err: any) {
    console.log(label, "throw", err.name, err.message);
  }
}

// --- Array.prototype: non-callable callbacks throw ---
log("array map number", () => [1, 2, 3].map(5 as any));
log("array forEach undefined", () => [1, 2, 3].forEach(undefined as any));
log("array filter string", () => [1, 2, 3].filter("x" as any));
log("array reduce number", () => [1, 2, 3].reduce(5 as any));
log("array reduceRight number", () => [1, 2, 3].reduceRight(5 as any));
log("array some number", () => [1, 2, 3].some(5 as any));
log("array every number", () => [1, 2, 3].every(5 as any));
log("array find number", () => [1, 2, 3].find(5 as any));
log("array findIndex number", () => [1, 2, 3].findIndex(5 as any));
log("array findLast number", () => [1, 2, 3].findLast(5 as any));
log("array findLastIndex number", () => [1, 2, 3].findLastIndex(5 as any));
log("array flatMap number", () => [1, 2, 3].flatMap(5 as any));
log("array map null", () => [1, 2, 3].map(null as any));
log("array map boolean", () => [1, 2, 3].map(true as any));
log("array map object", () => [1, 2, 3].map({} as any));
log("array sort string", () => [3, 1, 2].sort("x" as any));
log("array sort number", () => [3, 1, 2].sort(5 as any));

// --- %TypedArray%: most methods share Array's message; map differs ---
log("typedarray map number", () => new Int32Array([1]).map(7 as any));
log("typedarray forEach string", () => new Int32Array([1]).forEach("x" as any));
log("typedarray filter undefined", () => new Float64Array([1]).filter(undefined as any));
log("typedarray reduce number", () => new Int32Array([1]).reduce(5 as any));
log("typedarray some number", () => new Int32Array([1]).some(5 as any));
log("typedarray sort string", () => new Int32Array([3, 1, 2]).sort("x" as any));

// --- Valid callbacks must NOT throw ---
log("array map valid", () => [1, 2, 3].map((x) => x * 2));
log("array filter valid", () => [1, 2, 3].filter((x) => x > 1));
log("array reduce valid", () => [1, 2, 3].reduce((a, b) => a + b, 0));
log("array forEach valid", () => {
  let sum = 0;
  [1, 2, 3].forEach((x) => {
    sum += x;
  });
  return sum;
});
log("array sort valid", () => [3, 1, 2].sort((a, b) => a - b));
