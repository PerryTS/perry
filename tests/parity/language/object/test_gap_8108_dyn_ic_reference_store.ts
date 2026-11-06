// #8108: the inline dynamic-key write IC now stores REFERENCE values (object /
// string / bigint) through a barriered inline arm instead of diverting them to
// the outlined helper. Receivers are laundered through an `any[]` so the store
// site is the opaque same-receiver PutValue that lowers to
// `lower_put_value_dyn_ic_inline`, and every RHS reaches a safepoint so the
// static write PIC declines and the dynamic IC is what runs.

const bag: any[] = [];
function stash(o: any): number { bag.push(o); return bag.length - 1; }
function id(v: any): any { return v; }
function tryWrite(i: number, k: string, v: any): string {
  const o: any = bag[i];
  try { o[k] = v; return "ok"; } catch (e: any) { return "throw:" + (e instanceof TypeError); }
}
function mk(i: number): any { return { a: i, b: i + 1, c: 0, d: 0 }; }

const out: string[] = [];

// Every value tag through ONE site.
{
  const i = stash(mk(1));
  const vals: any[] = [{ v: 1 }, "str", 12345678901234567890n, Symbol("s"), 5, true, null, undefined, 1.5, "x"];
  for (let n = 0; n < vals.length; n++) {
    const o: any = bag[i];
    o.a = id(vals[n]);
    out.push(typeof o.a + ":" + String(o.a === vals[n]));
  }
}

// Frozen / sealed / non-extensible / accessor / read-only receivers keep their
// semantic paths; the inline arm must never store into any of them.
{
  const f = stash(mk(4)); Object.freeze(bag[f]);
  out.push("frozen:" + tryWrite(f, "a", id({ x: 1 })) + ":" + JSON.stringify(bag[f].a));
  const se = stash(mk(5)); Object.seal(bag[se]);
  out.push("sealed:" + tryWrite(se, "a", id({ x: 2 })) + ":" + JSON.stringify(bag[se].a));
  const ne = stash(mk(6)); Object.preventExtensions(bag[ne]);
  out.push("noext:" + tryWrite(ne, "a", id({ x: 3 })) + "," + tryWrite(ne, "zz", id({ y: 4 })) +
    ":" + JSON.stringify(bag[ne].a) + "," + JSON.stringify(bag[ne].zz));
  let seen: any = null;
  const ac: any = {};
  Object.defineProperty(ac, "a", { set(v: any) { seen = v; }, get() { return seen; }, configurable: true });
  out.push("accessor:" + tryWrite(stash(ac), "a", id({ x: 5 })) + ":" + JSON.stringify(seen));
  const ro: any = {};
  Object.defineProperty(ro, "a", { value: 1, writable: false, configurable: true });
  out.push("readonly:" + tryWrite(stash(ro), "a", id({ x: 6 })) + ":" + JSON.stringify(ro.a));
}

// Inherited setter, proxy trap, arrays and typed arrays.
{
  const proto: any = {}; let captured: any = null;
  Object.defineProperty(proto, "hook", { set(v: any) { captured = v; }, get() { return captured; }, configurable: true });
  const child = stash(Object.create(proto));
  out.push("inherited:" + tryWrite(child, "hook", id({ z: 9 })) + ":" + JSON.stringify(captured) +
    "," + String(Object.prototype.hasOwnProperty.call(bag[child], "hook")));
  const traps: string[] = [];
  const px = stash(new Proxy({ a: 0 } as any, { set(t: any, k: any, v: any) { traps.push(String(k)); t[k] = v; return true; } }));
  out.push("proxy:" + tryWrite(px, "a", id({ p: 1 })) + ":" + traps.join(",") + ":" + JSON.stringify(bag[px].a));
  const arr = stash([1, 2, 3]);
  out.push("arr:" + tryWrite(arr, "x", id({ n: 1 })) + "," + tryWrite(arr, "1", id({ n: 2 })) +
    ":" + JSON.stringify(bag[arr].x) + "," + JSON.stringify(bag[arr][1]) + "," + bag[arr].length);
  const ta = stash(new Uint8Array(4));
  out.push("ta:" + tryWrite(ta, "0", id(255)) + "," + tryWrite(ta, "tag", id("t")) +
    ":" + bag[ta][0] + "," + bag[ta].tag);
}

// A shape transition, and a throwing RHS that must leave the slot untouched.
{
  const t = stash(mk(7));
  for (let n = 0; n < 8; n++) {
    const o: any = bag[t];
    o.a = id({ i: n });
    if (n === 3) o.extra = id("added");
    if (n === 5) delete o.b;
  }
  out.push("transition:" + JSON.stringify(bag[t]));
  const i = stash(mk(9));
  const before = bag[i].a;
  try {
    const o: any = bag[i];
    o.a = id(((): any => { throw new RangeError("boom"); })());
    out.push("throwrhs:nothrow");
  } catch (e: any) { out.push("throwrhs:" + (e instanceof RangeError) + ":" + String(bag[i].a === before)); }
}

// Target, key and RHS evaluate exactly once, in spec order.
{
  const order: string[] = [];
  const objs: any[] = [mk(10)];
  function tgt(): any { order.push("t"); return objs[0]; }
  function ky(): string { order.push("k"); return "a"; }
  function rhs(): any { order.push("v"); return { ok: 1 }; }
  tgt()[ky()] = rhs();
  out.push("order:" + order.join("") + ":" + JSON.stringify(objs[0].a));
}

// Volume: old->young edges written from a producer reached through an `any[]`,
// so it cannot be inlined into a rooted temp — which is what keeps this loop on
// the barriered inline arm rather than the static write PIC.
{
  const producers: any[] = [
    (n: number): any => ({ r: n, s: "v" + n }),
    (n: number): any => "s" + n + "-" + (n * 7),
  ];
  const keep: any[] = [];
  for (let n = 0; n < 600; n++) keep.push(mk(n));
  for (let round = 0; round < 12; round++) {
    const produce: any = producers[round & 1];
    for (let n = 0; n < keep.length; n++) {
      const o: any = keep[n];
      o.c = produce(n);
      o.d = produce(n + 1);
    }
  }
  let s = 0;
  for (let n = 0; n < keep.length; n++) {
    const c: any = keep[n].c;
    const d: any = keep[n].d;
    s += (typeof c === "string" ? c.length : c.r) + (typeof d === "string" ? d.length : d.r);
  }
  out.push("volume:" + s);
}

console.log(out.join("\n"));
