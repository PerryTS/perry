// `a * b` over two int32 values whose product passes 2^53 rounds as a
// double, so `a * b === c * d` is no integer compare (`expr::int_product`:
// two rounding sides). Under PERRY_FACTS_SABOTAGE=intprod every leaf of an
// integer tree is claimed to be `0`, so the test is admitted with exact
// intervals: the leaf witness must report the first value that is not `0`.
const z = process.argv.length > 100 ? 1 : 0;
function same(a: number, b: number, c: number): boolean {
  const x = a | 0, y = b | 0, w = c | 0;
  return x * x === y * w;
}
console.log(same(94906267 + z, 94906266, 94906268), same(3 + z, 1, 9), same(3, 2, 4));
