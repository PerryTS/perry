// toReversed / toSorted / toSpliced / with return new arrays and leave the receiver
// alone; copyWithin edits a module-level and a function-local array in place;
// reduceRight folds from the end, with and without an initial value.
const a = [3, 1, 2];
console.log(a.toReversed(), a.toSorted(), a.toSorted((x, y) => y - x), a);
console.log(a.toSpliced(1, 1, 9, 8), a.toSpliced(1), a.toSpliced(0, 0, 5), a.with(0, 7), a.with(-1, 4), a);
const b = [1, 2, 3, 4, 5];
console.log(b.copyWithin(0, 3), b);
function local(): number[] {
  const c = [1, 2, 3, 4, 5];
  c.copyWithin(1, 0, 2);
  return c;
}
console.log(local());
console.log([1, 2, 3].reduceRight((acc, x) => acc + x, ""), [1, 2, 3].reduceRight((acc, x) => acc * 10 + x));
try { a.with(5, 0); } catch (e) { console.log(e instanceof RangeError); }
