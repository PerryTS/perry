// Per-unit facts (decision 151): `Object.prototype` is one object for the
// whole program, and no contract describes it. `patch` (a unit of its own)
// replaces `hasOwnProperty` there with a function that writes a string into
// its receiver's `n`; the module scope captures it and calls it on its own
// literal. The literal's unit must see that write: claiming `cache.n` a
// readonly number made `twice()` read the string as a number ("sz" instead
// of NaN).
function patch(): void {
  (Object.prototype as any).hasOwnProperty = function (this: any, k: string): boolean {
    this.n = "s" + k;
    return true;
  };
}
patch();
var hop = Object.prototype.hasOwnProperty;
var cache = { n: 1, m: 2 };
function check(k: string): boolean {
  return hop.call(cache, k);
}
function twice(): number {
  return cache.n * 2 + cache.m;
}
let acc = 0;
for (let i = 0; i < 3; i++) acc += twice();
console.log(check("z"), acc, twice(), typeof cache.n, cache.n);
