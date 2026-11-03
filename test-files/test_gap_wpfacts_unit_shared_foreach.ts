// Per-unit facts (decision 151): `add` (a unit of its own) puts
// `Object.prototype` into the module scope's list; `patch` runs a function
// on each element (the runtime calls it), and that function stores into
// its parameter: the store lands in `Object.prototype`.
var list: any[] = [];
function add(x: any): void {
  list.push(x);
}
add(Object.prototype);
function patch(): void {
  list.forEach(function (p: any): void {
    p.toLocaleString = function (this: any): string {
      this.u = "w";
      return "x";
    };
  });
}
patch();
var tls = Object.prototype.toLocaleString;
var pair = { u: 5, v: 6 };
function total(): number {
  return pair.u * 2 + pair.v;
}
console.log(tls.call(pair), total(), typeof pair.u);
