// Per-unit facts (decision 151): `Object.prototype` is one object for the
// whole program, and no contract describes it.
// The module scope hands it to a helper of another unit that stores into
// its parameter (⊤ there): the store lands in `Object.prototype`.
function fill(t: any, k: string, v: any): void {
  t[k] = v;
}
try {
  fill(Object.prototype, "toLocaleString", function (this: any): string {
    this.u = "w";
    return "x";
  });
} catch (e) { console.log("DEC 250: " + (e as Error).message); }
var tls = Object.prototype.toLocaleString;
var pair = { u: 5, v: 6 };
function total(): number {
  return pair.u * 2 + pair.v;
}
console.log(tls.call(pair), total(), typeof pair.u);
