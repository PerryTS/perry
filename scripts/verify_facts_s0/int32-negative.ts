// `k` is never an int32 (3000000000 + i). Under PERRY_FACTS_SABOTAGE=range the
// `+` interval rule drops its right operand, so the facts claim `k` is one:
// the store witness of the int32 proof must report it.
function sum(arr: number[]): number {
  let total = 0;
  for (let i = 0; i < 4; i++) {
    const k = i + 3000000000;
    total += (arr[k & 1] ?? 0) + k;
  }
  return total;
}
console.log(sum([1, 2, 3]));
