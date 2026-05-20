// Issue #1199: `console.dir(value, { depth: N })` should match Node's depth
// truncation. The default depth is 2; `depth: 0` collapses all nested objects
// to `[Object]` / `[Array]`; finite values cap nesting at N levels.

console.dir({ foo: { bar: { baz: true } } }, { depth: 0 });
console.dir({ foo: { bar: { baz: true } } }, { depth: 1 });

const deeply = {
  a: {
    b: {
      c: {
        d: 1,
      },
    },
  },
};

console.dir(deeply, { depth: 0 });
console.dir(deeply, { depth: 1 });
console.dir(deeply, { depth: 2 });

const nestedArr = [[[[1]]]];
console.dir(nestedArr, { depth: 0 });
console.dir(nestedArr, { depth: 1 });
