// Controls of test_typed_array_method_sites.ts: receivers the whole-program facts prove typed arrays keep their
// typed-array lowering (a parameter, a field, a module-scope local; folded and unfolded method nodes).
// `PERRY_METHOD_SITE_DIAG=1` prints `verdict=proven` for each of the twelve lowered sites of this file.
function only(a: Float64Array): number { a.fill(1.5); a.reverse(); return a.at(-1)! + a.indexOf(1.5) + a.subarray(1).length; }
{ const a = new Float64Array(1000); let s = 0; for (let i = 0; i < 50; i++) s += only(a); console.log("control", s, a.length); }
class Ring { buf: Float32Array = new Float32Array(8); n = 0; put(v: number): void { this.buf.fill(v, this.n, this.n + 1); this.n = (this.n + 1) & 7; } sum(): number { return this.buf.reduce((p, x) => p + x, 0); } }
{ const r = new Ring(); for (let i = 0; i < 20; i++) r.put(i); console.log("ring", r.sum(), r.buf.join(",")); }
const u = new Int32Array(4); console.log(u.fill(3).join("-"), u.at(1));
