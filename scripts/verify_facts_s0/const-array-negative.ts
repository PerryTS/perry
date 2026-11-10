// `A` is a literal of Numbers, and `poke` stores into it before the walk,
// so it is not immutable: the walk reads its slots.
// Under PERRY_FACTS_SABOTAGE=arrconst a write to an Array is not recorded,
// so the facts keep A's birth contents [1, 2, 3, 4] and the walk is
// unrolled over those constants. Its witness must report the false proof
// at the slot that no longer holds its birth value.
const A = [1, 2, 3, 4];
function poke(a: number[], k: number): void {
  a[k] = 40;
}
const z = process.argv.length > 100 ? 1 : 0;
poke(A, 2 + z);
let s = 0;
for (const x of A) s += x;
console.log(s);
