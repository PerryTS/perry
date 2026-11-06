// A setter defineProperty-d on a shared root prototype intercepts key adds on
// every ordinary receiver whose chain reaches it: literals, class instances,
// Object.create objects and functions, through static and dynamic keys,
// including add paths warmed before the setter existed. DEC 250 locks
// Object.prototype, so the root is a user object R that every receiver's
// chain reaches before Object.prototype.

const log: string[] = [];
const R: any = {};

function lit(v: number) {
  const o: any = Object.setPrototypeOf({ a: 1 }, R);
  o.zz = v;
  return o;
}
class K {
  b = 2;
}
Object.setPrototypeOf(K.prototype, R);
function inst(v: number) {
  const o: any = new K();
  o.zz = v;
  return o;
}
function created(v: number) {
  const o: any = Object.create(Object.setPrototypeOf({ p: 1 }, R));
  o.zz = v;
  return o;
}
function dyn(v: number, key: string) {
  const o: any = Object.setPrototypeOf({ c: 3 }, R);
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

Object.defineProperty(R, "zz", {
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
Object.setPrototypeOf(fn, R);
fn.zz = 15;
console.log("after:", log.join(","));
console.log(Object.keys(a).join(","), Object.keys(b).join(","), Object.keys(c).join(","), Object.keys(d).join(","));
console.log(a.zz, b.zz, c.zz, d.zz, fn.zz);

// A non-writable data property on the root blocks the add.
Object.defineProperty(R, "frozenKey", { value: "proto", writable: false, configurable: true });
const e: any = Object.setPrototypeOf({ e: 1 }, R);
try {
  e.frozenKey = "own";
  console.log("assigned", e.frozenKey, Object.keys(e).join(","));
} catch (err) {
  console.log("threw", (err as Error).constructor.name, e.frozenKey, Object.keys(e).join(","));
}

delete R.zz;
delete R.frozenKey;
const f2 = lit(20);
console.log("removed:", Object.keys(f2).join(","), f2.zz, log.length);
