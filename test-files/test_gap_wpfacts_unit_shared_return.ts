// Per-unit facts (decision 151): `proto` (a unit of its own) returns
// `Object.prototype`; `install` calls it through an array element (unknown
// there) and stores into the result: the store lands in
// `Object.prototype`.
function proto(): any {
  return Object.prototype;
}
var fns: any[] = [proto];
function install(): void {
  var p = fns[0]();
  p.toLocaleString = function (this: any): string {
    this.u = "w";
    return "x";
  };
}
install();
var tls = Object.prototype.toLocaleString;
var pair = { u: 5, v: 6 };
function total(): number {
  return pair.u * 2 + pair.v;
}
console.log(tls.call(pair), total(), typeof pair.u);
