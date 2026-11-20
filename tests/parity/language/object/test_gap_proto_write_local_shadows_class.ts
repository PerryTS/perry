// `<ident>.prototype.<m> = fn` where the ident is a lexical LOCAL (an ES5
// constructor function) that shadows a same-named module-scope class: the
// write must land on the local function's prototype, not register onto the
// unrelated class. Vendored eventemitter3 (`function s(){}` +
// `s.prototype.emit = fn`) inside a turbopack chunk that also has minified
// `class s {…}` declarations lost its whole prototype surface — every
// subclass (p-queue's PQueue) inherited nothing and Next page renders died
// on `this.emit(...)`.
class s {
  tracer(): string { return "tracer"; }
}
function makeEE(): any {
  function s(this: any) { this._events = {}; }
  (s as any).prototype.on = function (this: any, n: string, f: any) {
    (this._events[n] = this._events[n] || []).push(f);
    return this;
  };
  (s as any).prototype.emit = function (this: any, n: string) {
    (this._events[n] || []).forEach((f: any) => f());
    return true;
  };
  return s;
}
const EE = makeEE();
console.log("[1] proto surface:", typeof EE.prototype.on, typeof EE.prototype.emit);
// subclass over the ES5 function through a dynamic parent
const Q: any = class extends EE {
  constructor() { super(); this.ok = 1; }
  kick() { return this.emit("x"); }
};
const q = new Q();
let hits = 0;
q.on("x", () => { hits++; });
console.log("[2] inherited:", typeof q.on, typeof q.emit, q.kick(), hits);
// the module class is untouched
const t = new s();
console.log("[3] class s intact:", t.tracer(), typeof (s.prototype as any).emit);
