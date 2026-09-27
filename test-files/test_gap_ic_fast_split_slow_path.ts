// S2 (deferred-collection RFC): the slow arm of every fast/slow split runs
// user code that allocates heavily -- a getter, a Proxy trap, a setter, a
// `toString`, a not-callable throw -- while the caller holds live heap values
// that it reads afterwards. Under a moving collection those values must come
// back relocated. The Rust test `ic_fast_split_slow_path_evacuation.rs`
// compiles this file with PERRY_FULL_OUTLINE_IC=1 (where the IC splits apply)
// and runs it with a seeded evacuating schedule.
export {};

function churn(n: number): number {
  let sink = 0;
  for (let i = 0; i < n; i++) {
    const t = { i, s: "c" + i, a: [i, i + 1] };
    sink += t.a[1] - t.i;
  }
  return sink;
}

const N = 3000;

class Point {
  x: any;
  constructor(v: any) {
    this.x = v;
  }
}

function readGeneric(o: any): string {
  const keep = { tag: "keep-g", arr: [1, 2, 3] };
  const v = o.foo;
  return keep.tag + ":" + keep.arr.join("") + ":" + v.val;
}

function readField(p: Point): string {
  const keep = { tag: "keep-cg", n: 7 };
  const v = p.x;
  return keep.tag + keep.n + ":" + v.val;
}

function writeField(p: Point, v: any): string {
  const keep = { tag: "keep-cs", arr: [4, 5] };
  p.x = v;
  return keep.tag + keep.arr.join("");
}

function tmpl(x: any): string {
  const keep = { q: "keep-t", arr: [6] };
  const s = `${x}`;
  return keep.q + keep.arr[0] + ":" + s;
}

function callIt(f: any): string {
  const keep = { z: "keep-c", arr: [8, 9] };
  try {
    f(1);
  } catch (e) {
    churn(N);
    return "caught:" + (e instanceof TypeError) + ":" + keep.z + keep.arr.join("");
  }
  return "no-throw:" + keep.z;
}

// Generic read: a plain data property (the fast hit, twice so the site is
// primed), then a getter and a Proxy trap on the slow arm.
const data = { foo: { val: 1 } };
const getter = {
  get foo() {
    churn(N);
    return { val: 2 };
  },
};
const proxy = new Proxy({} as any, {
  get(_t: any, k: any) {
    churn(N);
    return { val: k === "foo" ? 3 : -1 };
  },
});
for (const o of [data, data, getter, proxy, data]) console.log(readGeneric(o));

// Class field: a plain instance (the guard passes) and one whose `x` is an
// own accessor (the guard fails; the by-name fallback runs the accessor).
const plain = new Point({ val: 10 });
const accessor = new Point({ val: 0 });
let stored: any = null;
Object.defineProperty(accessor, "x", {
  get() {
    churn(N);
    return { val: 20 };
  },
  set(v: any) {
    churn(N);
    stored = v;
  },
  configurable: true,
});
for (const p of [plain, plain, accessor, plain]) console.log(readField(p));
console.log(writeField(plain, { val: 11 }), plain.x.val);
console.log(writeField(accessor, { val: 21 }), stored.val);
console.log(writeField(plain, { val: 12 }), plain.x.val);

// Template coercion: a string (inline), a number and an object whose
// `toString` allocates (cold arm).
const heavy = {
  toString() {
    churn(N);
    return "T";
  },
};
for (const x of ["s", 42, heavy, "s2"]) console.log(tmpl(x));

// Dynamic callee: a function (inline unbox), then a number (cold arm throws).
console.log(callIt((n: number) => n));
console.log(callIt(42));
console.log(callIt((n: number) => n + 1));
