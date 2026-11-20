// `d` is never an int32 (i + 3000000000). Its only int consumer is the strict
// equality test against `7` (N1: `demand::compare_demand`), so it is I32 as
// soon as its interval claims int32. Under PERRY_FACTS_SABOTAGE=range the `+`
// interval rule drops its right operand and claims exactly that: the store
// witness of the int32 proof must report it.
function count(n: number): number {
  let hits = 0;
  for (let i = 0; i < n; i++) {
    let d = i + 3000000000;
    if (d === 7) hits += 1;
    hits += d * 0.5;
  }
  return hits;
}
console.log(count(4));
