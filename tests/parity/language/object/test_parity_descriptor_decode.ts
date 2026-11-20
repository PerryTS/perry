const show = (o: any, k: string) => {
  const d = Object.getOwnPropertyDescriptor(o, k)!;
  console.log(k, d.value, typeof d.get, d.writable, d.enumerable, d.configurable);
};
const a: any = {};
Object.defineProperty(a, "p1", { value: 1 });
Object.defineProperty(a, "p2", { value: 2, writable: true, enumerable: true, configurable: true });
Object.defineProperty(a, "p3", { enumerable: true, get: () => 3 });
show(a, "p1"); show(a, "p2"); show(a, "p3"); console.log("p3-read", a.p3);

const dproto = { value: 42, enumerable: true };
Object.defineProperty(a, "inh", Object.create(dproto));
show(a, "inh");
let fired = 0;
const dacc = { get value() { fired++; return 7; }, enumerable: true };
Object.defineProperty(a, "acc", dacc);
show(a, "acc"); console.log("getter-fired", fired >= 1);
class DescLike { get value() { return 9; } }
Object.defineProperty(a, "cls", new DescLike());
show(a, "cls");

(Object.prototype as any).enumerable = true;
Object.defineProperty(a, "pol", { value: 5 });
show(a, "pol");
delete (Object.prototype as any).enumerable;
Object.defineProperty(a, "unpol", { value: 6 });
show(a, "unpol");

const b: any = {};
Object.defineProperty(b, "t", { value: 1, writable: true, enumerable: true, configurable: true });
Object.defineProperty(b, "t", { get: () => 2, configurable: true });
show(b, "t"); console.log("t-read", b.t);
Object.defineProperty(b, "t", { value: 3 });
show(b, "t");

const c: any = { x: 1 };
Object.freeze(c);
let threw = false;
try { Object.defineProperty(c, "x", { value: 2 }); } catch { threw = true; }
console.log("frozen-throws", threw, c.x);
