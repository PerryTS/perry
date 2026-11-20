// `new Array(2_000_000)` is longer than the runtime's dense bound
// (MAX_FRESH_DENSE_ARRAY_LENGTH): its length is 2,000,000 over an empty
// store. Under PERRY_FACTS_SABOTAGE=densebirth the facts prove `big[i]` as
// a read of a store holding every slot, and its witness must report the
// capacity the store lacks.
const big = new Array(2_000_000);
function at(i: number): number {
  return big[i] === undefined ? 1 : 0;
}
let n = 0;
for (let i = 0; i < 4; i++) n += at(i * 500_000);
console.log(n);
