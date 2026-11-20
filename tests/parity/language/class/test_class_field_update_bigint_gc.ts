// `o.f++` on a class field holding a BigInt: the postfix result is the old
// BigInt, which must survive the allocation of the new one even when that
// allocation collects.
class Counter {
  n: any = 0n;
  m: any = 10n ** 30n;
}
const c = new Counter();
const olds: bigint[] = [];
let junk: any[] = [];
for (let i = 0; i < 20000; i++) {
  const old = c.n++;
  const big = c.m--;
  if (i % 4000 === 0) olds.push(old, big);
  junk.push({ i, s: "x" + i });
  if (junk.length > 500) junk = [];
}
console.log(c.n, c.m, olds.join(","));

// A redeclared `var` whose second initializer is an ordinary call.
function make(): number[] { return [7, 8]; }
function redeclare(): number {
  var a = [1];
  var a = make();
  return a[0] + a[1];
}
console.log(redeclare());
