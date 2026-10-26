// Per-unit facts (decision 151): `Object.prototype` is one object for the
// whole program. `fire` (a unit of its own) calls what it reads out of
// `hooks` (unknown there) with it; the function the module scope
// registered stores into its parameter: the store lands in
// `Object.prototype`, and `pair.u` is written with a string.
var hooks: any[] = [];
function register(f: any): void {
  hooks.push(f);
}
register(function (p: any): void {
  p.toLocaleString = function (this: any): string {
    this.u = "w";
    return "x";
  };
});
function fire(): void {
  for (var i = 0; i < hooks.length; i++) {
    hooks[i](Object.prototype);
  }
}
fire();
var tls = Object.prototype.toLocaleString;
var pair = { u: 5, v: 6 };
function total(): number {
  return pair.u * 2 + pair.v;
}
console.log(tls.call(pair), total(), typeof pair.u);
