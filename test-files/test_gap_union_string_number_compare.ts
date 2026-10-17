// A value typed `string | number` is not a string: relational operators and
// `+` on two numbers in such a union must not compare or join their decimal
// strings. fninfer joins a string caller's facts into the parameter, which
// made `20 < 3` compare "20" < "3" (TypeScript's compareComparableValues).
function cmp(a: string | number | undefined, b: string | number | undefined): number {
  return a === b ? 0 : a === undefined ? -1 : b === undefined ? 1 : a < b ? -1 : 1;
}
export function compareValues(a: number | undefined, b: number | undefined): number {
  return cmp(a, b);
}
export function compareStrings(a: string | undefined, b: string | undefined): number {
  return cmp(a, b);
}
function join(a: string | number, b: string | number) {
  return a + b;
}
function ge(a: string | number, b: string | number) {
  return a >= b;
}
const nums: number[] = [20, 3, 100, 9, 10];
const out: string[] = [];
for (let i = 0; i + 1 < nums.length; i++) {
  out.push(String(compareValues(nums[i], nums[i + 1])));
  out.push(String(join(nums[i], nums[i + 1])));
  out.push(String(ge(nums[i], nums[i + 1])));
}
console.log(out.join(" "));
console.log(compareStrings("20", "3"), compareStrings("a", "b"), join("1", "2"), ge("10", "9"));
console.log(compareValues(undefined, 1), compareValues(1, undefined), compareValues(2, 2));
console.log(["b", "a", "c"].sort(compareStrings).join(), [20, 3, 100].sort(compareValues).join());
