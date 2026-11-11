// `a.slice(2)` is made with ArraySpeciesCreate: `a.constructor` holds an
// object whose `@@species` is `Object`, so the result is a Number object
// with index properties and `c` has no array rep. Under PERRY_FACTS_SABOTAGE=slicebirth a
// `slice` birth is claimed a plain Array whatever its species constructor:
// `c` is `Arr`, and the witness must report the header at the first read.
function probe(): number {
  const a: any = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
  if (process.argv.length > 0) {
    a.constructor = { [Symbol.species]: Object };
  }
  const c = a.slice(2);
  let s = 0;
  for (let i = 0; i < 8; i++) {
    const k = i;
    s = s + c[k];
  }
  return s;
}
console.log(probe());
