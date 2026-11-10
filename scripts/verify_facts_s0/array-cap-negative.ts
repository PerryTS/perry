// `new Array(2_000_000)` is longer than the runtime's dense bound
// (MAX_FRESH_DENSE_ARRAY_LENGTH): its length is 2,000,000 over an empty
// store, so wpfacts records no birth length for it. Under
// PERRY_FACTS_SABOTAGE=arrcap the birth length is recorded over the bound:
// `big[i]` is a proven read of a store holding every slot, and its witness
// must report the capacity the store lacks (capacity >= floor). (No `fill`:
// it would densify the store and make the claim true.)
const big = new Array<number>(2_000_000);
function at(i: number): number {
  return big[i] ?? -1;
}
console.log(at(5), at(1_999_999), big.length);
