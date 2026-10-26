// Per-unit facts (decision 151): `Object.prototype` is one object for the
// whole program, and no contract describes it. `P` holds it; `put` (a unit
// of its own) reads it through `P` and stores into it. The literal's unit
// must see that store: `cache.n` is written with a string.
var P: any = Object.prototype;
function put(): void {
  P.hasOwnProperty = function (this: any, k: string): boolean {
    this.n = "s" + k;
    return true;
  };
}
put();
var hop = Object.prototype.hasOwnProperty;
var cache = { n: 1, m: 2 };
function check(k: string): boolean {
  return hop.call(cache, k);
}
function twice(): number {
  return cache.n * 2 + cache.m;
}
console.log(check("z"), twice(), typeof cache.n);
