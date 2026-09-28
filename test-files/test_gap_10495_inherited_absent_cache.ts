// #10495 / #10497 / #10753 / #10877: reads that resolve on the prototype chain,
// or fall off its end, are cached per (receiver shape, key). Every case below
// reads the same site many times, mutates something the cached answer depends
// on part-way through, and prints what each read saw — so a cache that fails
// to invalidate prints a stale value where node prints the new one.

function readLoop(label: string, n: number, read: (i: number) => unknown, mutate: (i: number) => void) {
  const seen: string[] = [];
  let last = "";
  for (let i = 0; i < n; i++) {
    mutate(i);
    const v = read(i);
    const s = typeof v === "function" ? "fn:" + (v as any).name : String(v);
    if (s !== last) { seen.push(i + "=" + s); last = s; }
  }
  console.log(label + ": " + seen.join(" "));
}

// 1. Absent key on object literals, then Object.prototype gains / loses it.
{
  const bags: any[] = [{ a: 1 }, { a: 2 }, { a: 3 }];
  readLoop("literal absent -> Object.prototype add/delete", 40, (i) => bags[i % 3].zz1, (i) => {
    if (i === 10) (Object.prototype as any).zz1 = "proto";
    if (i === 20) delete (Object.prototype as any).zz1;
  });
}

// 2. Absent key, then a getter is defined on Object.prototype (must run every read).
{
  let calls = 0;
  const bags: any[] = [{ b: 1 }, { b: 2 }];
  readLoop("literal absent -> Object.prototype getter", 30, (i) => bags[i & 1].zz2, (i) => {
    if (i === 10) Object.defineProperty(Object.prototype, "zz2", { get() { calls++; return calls > 12 ? "late" : undefined; }, configurable: true });
    if (i === 25) delete (Object.prototype as any).zz2;
  });
  console.log("getter calls", calls);
}

// 3. The receiver gains the key as an own property, then loses it.
{
  const o: any = { c: 1 };
  readLoop("own add/delete over absent", 30, () => o.zz3, (i) => {
    if (i === 10) o.zz3 = "own";
    if (i === 20) delete o.zz3;
  });
}

// 4. setPrototypeOf on a literal receiver part-way through.
{
  const o: any = { d: 1 };
  readLoop("setPrototypeOf literal", 30, () => o.zz4, (i) => {
    if (i === 10) Object.setPrototypeOf(o, { zz4: "newproto" });
    if (i === 20) Object.setPrototypeOf(o, Object.prototype);
  });
}

// 5. A deep Object.create chain (8 levels): miss, then the far end gains the key,
//    then a middle level shadows it, then the middle level loses it again.
{
  let p: any = { base: 1 };
  const levels: any[] = [p];
  for (let d = 1; d < 8; d++) { p = Object.create(p); p["l" + d] = d; levels.push(p); }
  const rs: any[] = [];
  for (let q = 0; q < 4; q++) rs.push(Object.create(p));
  readLoop("deep chain miss/add/shadow", 50, (i) => rs[i & 3].zz5, (i) => {
    if (i === 10) levels[0].zz5 = "far";
    if (i === 20) levels[5].zz5 = "mid";
    if (i === 30) delete levels[5].zz5;
    if (i === 40) delete levels[0].zz5;
  });
  readLoop("deep chain inherited hit", 20, (i) => rs[i & 3].base, (i) => {
    if (i === 10) levels[0].base = 99;
  });
}

// 6. Computed-key reads (by-name path), absent then present on the prototype.
{
  const O: any = { a: 1, b: 2 };
  const keys = ["a", "zz6", "b", "zz7"];
  let sum = 0, undef = 0;
  for (let i = 0; i < 40; i++) {
    if (i === 20) (Object.prototype as any).zz6 = 100;
    const v = O[keys[i & 3]];
    if (v === undefined) undef++; else sum += v;
  }
  delete (Object.prototype as any).zz6;
  console.log("computed keys", sum, undef, O["zz6"]);
}

// 7. `constructor` on literals, class instances, Object.create and null-prototype
//    receivers.
{
  class K { k = 1; }
  class L { l = 1; get constructor2() { return 1; } }
  const xs: any[] = [{ a: 1 }, new K(), Object.create({ q: 1 }), new L(), Object.create(null)];
  const out: string[] = [];
  for (let i = 0; i < 25; i++) {
    const x = xs[i % 5];
    const c = x.constructor;
    if (i >= 20) out.push(c === undefined ? "undef" : c.name);
  }
  console.log("constructor", out.join(","));
}

// 8. Object.prototype methods read through literals stay the real ones, and a
//    replaced method is seen.
{
  const o: any = { e: 1 };
  const orig = Object.prototype.toString;
  readLoop("toString through literal", 20, () => o.toString === orig ? "orig" : "replaced", (i) => {
    if (i === 10) (Object.prototype as any).toString = function () { return "x"; };
  });
  (Object.prototype as any).toString = orig;
  console.log("hasOwnProperty", o.hasOwnProperty("e"), o.hasOwnProperty("zz"), typeof o.isPrototypeOf);
}

// 9. Functions: a missing property, then Function.prototype gains / loses it.
{
  function f1() { return 1; }
  const g1 = () => 2;
  const fns: any[] = [f1, g1, Object, Array];
  readLoop("function absent -> Function.prototype add/delete", 40, (i) => fns[i & 3].isBuffer, (i) => {
    if (i === 10) (Function.prototype as any).isBuffer = "fp";
    if (i === 20) delete (Function.prototype as any).isBuffer;
    if (i === 30) (Object.prototype as any).isBuffer = "op";
  });
  delete (Object.prototype as any).isBuffer;
  const objs: any[] = [{ a: 1 }, { b: 1 }];
  let bufs = 0;
  for (let i = 0; i < 20; i++) {
    const v = objs[i & 1];
    if (v.constructor && typeof v.constructor.isBuffer === "function" && v.constructor.isBuffer(v)) bufs++;
  }
  console.log("axios isBuffer", bufs);
}

// 10. Null-prototype receivers and an explicit __proto__ read.
{
  const n: any = Object.create(null);
  n.a = 1;
  let und = 0;
  for (let i = 0; i < 10; i++) if (n.toString === undefined && n.zz === undefined) und++;
  const o: any = { a: 1 };
  console.log("null proto", und, Object.getPrototypeOf(o) === Object.prototype, o.__proto__ === Object.prototype);
}

// 11. Class getters that return undefined must still run on every read.
{
  let hits = 0;
  class G { get maybe(): any { hits++; return undefined; } }
  const gs = [new G(), new G()];
  let und = 0;
  for (let i = 0; i < 30; i++) if (gs[i & 1].maybe === undefined) und++;
  console.log("class getter undefined", und, hits);
}

// 12. lru-cache's shape: destructuring defaults from an options bag that lacks
//     every key, then an options bag that has them.
{
  class Cache {
    ttl = 3; allowStale = false; v = 0;
    get(k: number, opts: any = {}) {
      const { allowStale = this.allowStale, ttl = this.ttl, status } = opts;
      this.v = (this.v + k + (allowStale ? 1 : 0) + ttl + (status ? 1 : 0)) | 0;
      return this.v;
    }
  }
  const c = new Cache();
  let a = 0;
  for (let i = 0; i < 30; i++) a = c.get(i, i < 20 ? {} : { allowStale: true, ttl: 10, status: 1 });
  (Object.prototype as any).ttl = 1000;
  a = c.get(1);
  delete (Object.prototype as any).ttl;
  a = c.get(1) + a;
  console.log("lru opts", a);
}

// 13. JSON-parsed objects read absent keys, then Object.prototype gains one.
{
  const rows: any[] = JSON.parse('[{"id":1},{"id":2},{"id":3}]');
  readLoop("json rows absent", 30, (i) => rows[i % 3].missing, (i) => {
    if (i === 15) (Object.prototype as any).missing = "m";
  });
  delete (Object.prototype as any).missing;
}
