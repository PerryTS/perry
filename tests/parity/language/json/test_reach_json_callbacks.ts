// A JSON.stringify replacer function is called with `this` = the holder and
// (key, value) for the root (its holder a fresh `{"": value}`) and for every
// property and element the serialization visits, after `toJSON(key)`; what
// it returns replaces the value and is visited in turn. A JSON.parse reviver
// is called the same way on the parsed values, and its result is stored into
// the holder. The whole-program solve only escaped the callback, so an object
// reached only through the serialization kept its proven Number fields while
// the callback stored a string into them (check mode `PERRY_VERIFY_FACTS=1`:
// "F2 kind (raw f64)"; default mode printed the string where Node computes
// NaN). Each case reads the field with typed code after the callback ran.

// Store through `this` (the holder).
{
  const p = { n: 1, m: 2 };
  JSON.stringify({ p, tag: "t" }, function (this: any, k: string, v: any) {
    if (k === "tag") this.p.n = "str";
    return v;
  });
  let acc: any = 0;
  for (let i = 0; i < 2; i++) acc = acc + p.n;
  console.log("this:", acc, p.m * 2);
}

// Store into the value argument.
{
  const p = { n: 1 };
  JSON.stringify({ p }, (k: string, v: any) => {
    if (k === "p") v.n = "two";
    return v;
  });
  console.log("value:", p.n * 2, typeof p.n);
}

// Kept in an array, a Map and an outer variable, stored later.
{
  const p = { n: 1 };
  const q = { n: 4 };
  const kept: any[] = [];
  const m = new Map<string, any>();
  let holder: any = null;
  JSON.stringify({ a: { p } }, (k: string, v: any) => {
    if (k === "p") kept.push(v);
    return v;
  });
  JSON.stringify([q], function (this: any, k: string, v: any) {
    if (k === "0") {
      m.set("x", v);
      holder = this;
    }
    return v;
  });
  kept[0].n = "late";
  const rd = (): number => p.n * 3;
  console.log("kept:", rd(), kept.length);
  m.get("x").n = "mapped";
  console.log("map:", q.n * 2, Array.isArray(holder));
  holder[0].n = 9;
  console.log("holder:", q.n * 2);
}

// Keys deleted and added (the shape changes); a typed helper reads it.
{
  const p = { n: 1, s: 2 };
  JSON.stringify({ p }, function (this: any, k: string, v: any) {
    if (k === "n") {
      delete this.s;
      this.z = "added";
    }
    return v;
  });
  console.log("shape:", p.s, (p as any).z, Object.keys(p).join(","));
  const q = { n: 1, s: 2 };
  const sum = (o: { n: number; s: number }) => o.n + o.s;
  JSON.stringify(q, function (this: any, k: string, v: any) {
    if (k === "n") this.s = "S";
    return v;
  });
  console.log("helper:", sum(q));
}

// Array elements, an int32 array at rest among them.
{
  const a = [1, 2, 3];
  JSON.stringify({ a }, function (this: any, k: string, v: any) {
    if (k === "1") this[1] = "two";
    return v;
  });
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  const b: number[] = [];
  for (let i = 0; i < 8; i++) b.push(i | 0);
  JSON.stringify(b, (k: string, v: any) => {
    if (Array.isArray(v)) v[3] = "three";
    return v;
  });
  let s: any = 0;
  for (let i = 0; i < b.length; i++) s = s + b[i];
  console.log("elements:", t, s);
}

// What the replacer returns is visited; what toJSON returns is the value.
{
  class Box {
    w: number;
    constructor() {
      this.w = 3;
    }
  }
  const made = new Box();
  const out = JSON.stringify({ k: 1 }, function (this: any, k: string, v: any) {
    if (k === "k") return made;
    if (k === "w") this.w = "www";
    return v;
  });
  const inner = { n: 2 };
  const t = {
    toJSON() {
      return { inner };
    },
  };
  JSON.stringify({ t }, (k: string, v: any) => {
    if (k === "inner") v.n = "x";
    return v;
  });
  console.log("returned:", out, made.w * 2, inner.n * 2);
}

// A class's toJSON that stores into `this`, beside a replacer.
{
  class T {
    n: number;
    constructor() {
      this.n = 1;
    }
    toJSON(key: string) {
      (this as any).n = "json:" + key;
      return { k: key };
    }
  }
  const t = new T();
  console.log("toJSON:", JSON.stringify({ t }, (k: string, v: any) => v), t.n * 2, typeof t.n);
}

// Bound, closure, method and async replacers.
{
  const p = { n: 1 };
  const ctx = { tag: "c" };
  function rep(this: any, k: string, v: any) {
    if (k === "p") v.n = this.tag + "!";
    return v;
  }
  JSON.stringify({ p }, rep.bind(ctx));
  const q = { n: 1 };
  const mk = (tag: string) => (k: string, v: any) => {
    if (k === "q") v.n = tag;
    return v;
  };
  JSON.stringify({ q }, mk("closure"));
  class R {
    rep(k: string, v: any) {
      if (k === "r") v.n = "m";
      return v;
    }
  }
  const r = { n: 5 };
  JSON.stringify({ r }, new R().rep);
  const s = { n: 1 };
  const as = JSON.stringify({ s }, async (k: string, v: any) => {
    if (k === "s") v.n = "async";
    return v;
  });
  console.log("kinds:", p.n * 2, q.n * 2, r.n * 2, as, s.n * 2);
}

// Nested objects and arrays read by another function; the root's holder.
{
  const deep = { n: 1, arr: [{ m: 2 }] };
  JSON.stringify({ a: { b: { deep } } }, function (this: any, k: string, v: any) {
    if (k === "m") this.m = "mm";
    if (k === "n") this.n = "nn";
    return v;
  });
  const f = (): number => deep.n * 2 + deep.arr[0].m * 2;
  const p = { n: 1 };
  let wrap: any = null;
  JSON.stringify(p, function (this: any, k: string, v: any) {
    if (k === "") wrap = this;
    return v;
  });
  wrap[""].n = "w";
  console.log("nested:", f(), p.n * 2, Object.keys(wrap).length);
}

// A replacer chosen at run time, with `space`; JSON.stringify as a value.
{
  const p = { n: 1 };
  const rep: any =
    process.argv.length > 0
      ? (k: string, v: any) => {
          if (k === "p") v.n = "c";
          return v;
        }
      : null;
  console.log("space:", JSON.stringify({ p }, rep, 2).length);
  const q = { n: 1 };
  const ser = JSON.stringify;
  ser({ q }, (k: string, v: any) => {
    if (k === "q") v.n = "alias";
    return v;
  });
  const r = { n: 1 };
  JSON.stringify.call(null, { r }, (k: string, v: any) => {
    if (k === "r") v.n = "call";
    return v;
  });
  console.log("value of:", p.n * 2, q.n * 2, r.n * 2);
}

// A getter's value is visited.
{
  const p = { n: 1 };
  const o = {
    get g() {
      return p;
    },
  };
  JSON.stringify(o, (k: string, v: any) => {
    if (k === "g") v.n = "getter";
    return v;
  });
  console.log("getter:", p.n * 2);
}

// Revivers.
{
  class P {
    n: number;
    constructor() {
      this.n = 1;
    }
  }
  const pp = new P();
  const o: any = JSON.parse('{"a":{"x":1},"b":3}', function (this: any, k: string, v: any) {
    if (k === "a") return pp;
    if (k === "b") this.a.n = "changed";
    return v;
  });
  const kept: any[] = [];
  const o2: any = JSON.parse('{"a":{"n":1},"b":2}', function (this: any, k: string, v: any) {
    if (k === "a") kept.push(v);
    if (k === "b") this.a.n = "rev";
    return v;
  });
  console.log("reviver:", pp.n * 2, o.a === pp, o2.a.n, kept[0] === o2.a);
}

// Controls: no replacer, null / undefined, an array, a replacer that reads.
{
  const p = { n: 1, a: [1, 2], s: "x" };
  const keys = ["p", "s"];
  let c = 0;
  console.log(
    "controls:",
    JSON.stringify({ p }),
    JSON.stringify({ p }, null),
    JSON.stringify({ p }, undefined, 1).length,
    JSON.stringify({ p }, ["p", "n"]),
    JSON.stringify({ p }, keys),
    JSON.stringify({ p }, (k: string, v: any) => {
      c++;
      return typeof v === "number" ? v * 10 : v;
    }),
    p.n * 2,
    p.a[0] + p.a[1],
    c,
  );
}
