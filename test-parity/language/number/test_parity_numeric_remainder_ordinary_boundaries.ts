// Read through arrays to exercise ordinary arithmetic, including conversions
// that produce LLVM poison before the checked operation freezes them.
const dividends: number[] = [
  -2147483649, -2147483648.5, -2147483648, -2147483647.5,
  2147483646.5, 2147483647, 2147483647.5, 2147483648,
  4294967296, -4294967296, 9007199254740991, -9007199254740991,
  Number.MIN_VALUE, -Number.MIN_VALUE, 0, -0,
  NaN, Infinity, -Infinity, 1e300, -1e300, 7, -7,
];
const divisors: number[] = [
  -2147483649, -2147483648, 2147483647, 2147483648,
  -3, -1, 0, -0, 1, 3, 2.5, -2.5,
  Number.MIN_VALUE, -Number.MIN_VALUE, NaN, Infinity, -Infinity,
];
function show(value: number): string {
  if (Object.is(value, -0)) return "-0";
  return String(value);
}
for (let i = 0; i < dividends.length; i++) {
  let row = "";
  for (let j = 0; j < divisors.length; j++) {
    row += (j === 0 ? "" : ",") + show(dividends[i] % divisors[j]);
  }
  console.log(row);
}
