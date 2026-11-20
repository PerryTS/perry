// `.length` read through a function, in a program that also has an anonymous
// `{ length: n }` object shape. An array's, a string's and a typed array's
// header starts with a word that an object keeps its class id in, so a
// one-element value must not be mistaken for the `{ length: n }` shape:
// `len([x])` has to answer 1, not the raw bits of whatever sits at the
// object's first field slot (5e-324, 0).
function len(a: any) {
  return a.length;
}

for (let n = 1; n <= 3; n++) {
  const sliced = Array.prototype.slice.call({ length: n });
  console.log(
    'n=' + n,
    'slice.call=' + len(sliced),
    'from=' + len(Array.from({ length: n })),
    'fill=' + len(new Array(n).fill(7)),
    'string=' + len('abcd'.slice(0, n)),
    'u8=' + len(new Uint8Array(n)),
    'again=' + len(sliced),
  );
}
console.log('literal ' + len([5]) + ' ' + len(Array.of(9)) + ' ' + len([5]));
