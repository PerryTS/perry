// A store site lets the compiler give the arrays and objects that reach it a
// layout that already lists the stored key. Before the first store the key
// is absent, so the store is an add: an inherited setter or readonly key on
// the prototype chain decides it, exactly as for an object without the layout.
function putArray(o: any, v: number) { o.rangeEnd = v; }
function putObject(o: any, v: number) { o.limit = v; }

let total = 0;
const a0: any = [];
console.log("array before", Object.hasOwn(a0, "rangeEnd"));
putArray(a0, 1);
console.log("array own add", a0.rangeEnd, Object.hasOwn(a0, "rangeEnd"));

Object.defineProperty(Array.prototype, "rangeEnd", {
  set(v: number) { total += v; },
  get() { return -1; },
  configurable: true,
});
const a1: any = [];
putArray(a1, 8);
console.log("array setter", total, Object.hasOwn(a1, "rangeEnd"), a1.rangeEnd);
putArray(a0, 2);
console.log("array own overwrite", a0.rangeEnd, total);
delete (Array.prototype as any).rangeEnd;

Object.defineProperty(Array.prototype, "rangeEnd", { value: 5, writable: false, configurable: true });
const a2: any = [];
try { putArray(a2, 9); } catch (e: any) { console.log("array readonly threw", e instanceof TypeError); }
console.log("array readonly", a2.rangeEnd, Object.hasOwn(a2, "rangeEnd"));
delete (Array.prototype as any).rangeEnd;

const o0: any = {};
putObject(o0, 1);
class Base { set limit(v: number) { total += 100 * v; } }
const o1: any = new Base();
putObject(o1, 3);
console.log("object setter", total, Object.hasOwn(o1, "limit"));
const proto: any = {};
Object.defineProperty(proto, "limit", { set(v: number) { total += 1000 * v; } });
const o2: any = Object.create(proto);
putObject(o2, 2);
console.log("object create setter", total, Object.hasOwn(o2, "limit"), o0.limit);
