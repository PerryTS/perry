// `A` is an immutable literal with an elision: a hole at index 1. `xs`
// selects it or `B`, both of fixed length 4, so the facts give `xs` an
// immutable `Arr` that is not dense, and the for-of walk reads each element
// through the hole test.
// Under PERRY_FACTS_SABOTAGE=walkdense an immutable Array of fixed length
// counts as dense, so the walk is indexed and reads slot 1 as an element.
// Its witness must report the hole below the length.
const A = [1, , 3, 4];
const B = [5, 6, 7, 8];
function sum(k: number): number {
  const xs = k === 0 ? A : B;
  let s = 0;
  for (const v of xs) s += v === undefined ? 9 : v;
  return s;
}
const z = process.argv.length > 100 ? 1 : 0;
console.log(sum(z), sum(1));
