// Per-unit facts (decision 151): `each` (a unit of its own) calls its
// function parameter with `Object.prototype`; the module scope's function
// stores into its parameter: the store lands in `Object.prototype`, and
// `pair.u` is written with a string.
function each(f: any): void {
  f(Object.prototype);
}
each(function (p: any): void {
  p.toLocaleString = function (this: any): string {
    this.u = "w";
    return "x";
  };
});
var tls = Object.prototype.toLocaleString;
var pair = { u: 5, v: 6 };
function total(): number {
  return pair.u * 2 + pair.v;
}
console.log(tls.call(pair), total(), typeof pair.u);
