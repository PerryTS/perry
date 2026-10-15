// A setter defineProperty-d on Object.prototype intercepts key adds on every
// ordinary receiver whose chain reaches it: literals, class instances,
// Object.create objects and functions, through static and dynamic keys,
// including add paths warmed before the setter existed.

const log: string[] = [];

function lit(v: number) {
  const o: any = { a: 1 };
  o.zz = v;
  return o;
}
class K {
  b = 2;
}
function inst(v: number) {
  const o: any = new K();
  o.zz = v;
  return o;
}
function created(v: number) {
  const o: any = Object.create({ p: 1 });
  o.zz = v;
  return o;
}
function dyn(v: number, key: string) {
  const o: any = { c: 3 };
  o[key] = v;
  return o;
}
for (let i = 0; i < 50; i++) {
  lit(i);
  inst(i);
  created(i);
  dyn(i, "zz");
}
console.log("before:", Object.keys(lit(1)).join(","), Object.keys(inst(1)).join(","));

Object.defineProperty(Object.prototype, "zz", {
  set(v: number) {
    log.push("OP.zz=" + v);
  },
  get() {
    return "inherited";
  },
  configurable: true,
});

const a = lit(10);
const b = inst(11);
const c = created(12);
const d = dyn(13, "zz");
const fn: any = function f() {};
fn.zz = 15;
console.log("after:", log.join(","));
console.log(Object.keys(a).join(","), Object.keys(b).join(","), Object.keys(c).join(","), Object.keys(d).join(","));
console.log(a.zz, b.zz, c.zz, d.zz, fn.zz);

// A non-writable data property on Object.prototype blocks the add.
Object.defineProperty(Object.prototype, "frozenKey", { value: "proto", writable: false, configurable: true });
const e: any = { e: 1 };
try {
  e.frozenKey = "own";
  console.log("assigned", e.frozenKey, Object.keys(e).join(","));
} catch (err) {
  console.log("threw", (err as Error).constructor.name, e.frozenKey, Object.keys(e).join(","));
}

delete (Object.prototype as any).zz;
delete (Object.prototype as any).frozenKey;
const f2 = lit(20);
console.log("removed:", Object.keys(f2).join(","), f2.zz, log.length);
