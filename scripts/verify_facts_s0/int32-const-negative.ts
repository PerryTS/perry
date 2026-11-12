// `k` is never an int32 (i + 3000000000) and is read once, as a value: no
// use demands an int, so the facts leave it an F64 local. Under
// PERRY_FACTS_SABOTAGE=range the `+` interval rule drops its right operand
// and claims `k` in [0, 7]: the local joins the proven int32 `let`s, gets
// the collectors' i32 slot, and `return k` reads that slot. The store
// witness of the int32 proof must report the initializer (it was lowered
// natively and seeded the slot untested).
const RUN = Math.random() < 2 ? 1 : 0;
function g(i: number): number {
  const k = i + 3000000000;
  return k;
}
function f(i: number): number {
  return g(i);
}
const outv: number[] = [];
for (let i = 0; i < 4 * RUN; i++) outv.push(f(i));
console.log(outv.join(","));
