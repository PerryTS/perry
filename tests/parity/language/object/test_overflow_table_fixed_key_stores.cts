// Stores and reads of a fixed key (one the object's layout names) and of a
// table-held key on objects that carry an overflow table: a literal or class
// instance that gained a key through a computed store, lost one through
// delete, or had one redefined. The store site `o.a = v` must honour every
// attribute the table owns: frozen, sealed, non-writable, accessor, deleted,
// in sloppy and in strict code.
const Z: string = String.fromCharCode(122); // "z", not a compile-time key
const A: string = String.fromCharCode(97); // "a"
const out: string[] = [];
function rd(o: any): any {
  return o.a;
}
function rdb(o: any): any {
  return o.b;
}
function wr(o: any, v: any): string {
  // sloppy (a .cts file is a CommonJS script): a refused store is silent
  try {
    o.a = v;
    return "ok:" + String(o.a);
  } catch (e: any) {
    return "threw:" + e.constructor.name;
  }
}
function wrs(o: any, v: any): string {
  "use strict";
  try {
    o.a = v;
    return "ok:" + String(o.a);
  } catch (e: any) {
    return "threw:" + e.constructor.name;
  }
}
function wrz(o: any, v: any): string {
  "use strict";
  try {
    o.z = v;
    return "ok:" + String(o.z);
  } catch (e: any) {
    return "threw:" + e.constructor.name;
  }
}
function mk(i: number): any {
  const o: any = { a: i, b: i + 1, c: i + 2, d: i + 3 };
  o[Z] = 9;
  return o;
}
class P {
  a: number;
  b: number;
  constructor(i: number) {
    this.a = i;
    this.b = i + 1;
  }
}
function mkc(i: number): any {
  const o: any = new P(i);
  o[Z] = 9;
  return o;
}
function mkdyn(i: number): any {
  const o: any = {};
  o[A] = i;
  o[Z] = i + 1;
  return o;
}
function desc(o: any, k: string): string {
  const d = Object.getOwnPropertyDescriptor(o, k);
  if (!d) return "none";
  return (
    ("value" in d ? "v=" + String(d.value) : "acc") +
    (d.writable ? "W" : "-") +
    (d.enumerable ? "E" : "-") +
    (d.configurable ? "C" : "-")
  );
}
function both(tag: string, make: (i: number) => any, prep: (o: any) => void): void {
  const o = make(1);
  prep(o);
  const p = make(2);
  prep(p);
  out.push(tag + " sloppy " + wr(o, 100) + " " + desc(o, "a") + " " + Object.keys(o).join(","));
  out.push(tag + " strict " + wrs(p, 200) + " " + desc(p, "a") + " " + Object.keys(p).join(","));
}
const makers: [string, (i: number) => any][] = [
  ["lit", mk],
  ["cls", mkc],
  ["dyn", mkdyn],
];
for (const [name, make] of makers) {
  // warm the sites on plain table objects first
  for (let i = 0; i < 40; i++) {
    const w = make(i);
    wr(w, i);
    wrs(w, i);
    wrz(w, i);
  }
  both(name + " plain", make, () => {});
  both(name + " frozen", make, (o) => Object.freeze(o));
  both(name + " sealed", make, (o) => Object.seal(o));
  both(name + " noext", make, (o) => Object.preventExtensions(o));
  both(name + " nonwritable", make, (o) => Object.defineProperty(o, "a", { writable: false }));
  both(name + " nonwritable-other", make, (o) => Object.defineProperty(o, "z", { writable: false }));
  both(name + " nonwritable-b", make, (o) => {
    if ("b" in o) Object.defineProperty(o, "b", { writable: false });
  });
  both(name + " getter-only", make, (o) =>
    Object.defineProperty(o, "a", {
      get() {
        return "G";
      },
      configurable: true,
    }),
  );
  let seen = 0;
  both(name + " accessor", make, (o) =>
    Object.defineProperty(o, "a", {
      get() {
        return "G" + seen;
      },
      set(v: number) {
        seen += v;
      },
      configurable: true,
      enumerable: true,
    }),
  );
  out.push(name + " accessor seen " + seen);
  both(name + " accessor-then-data", make, (o) => {
    Object.defineProperty(o, "a", { get: () => 1, configurable: true });
    Object.defineProperty(o, "a", { value: 5, writable: true, configurable: true });
  });
  both(name + " accessor-then-readonly", make, (o) => {
    Object.defineProperty(o, "a", { get: () => 1, configurable: true });
    Object.defineProperty(o, "a", { value: 5, writable: false, configurable: true });
  });
  both(name + " deleted", make, (o) => {
    delete o.a;
  });
  both(name + " deleted-frozen", make, (o) => {
    delete o.a;
    Object.freeze(o);
  });
  both(name + " deleted-noext", make, (o) => {
    delete o.a;
    Object.preventExtensions(o);
  });
  both(name + " deleted-readded", make, (o) => {
    delete o.a;
    o[A] = 7;
  });
  both(name + " deleted-readded-readonly", make, (o) => {
    delete o.a;
    o[A] = 7;
    Object.defineProperty(o, "a", { writable: false });
  });
  let pseen = 0;
  both(name + " deleted-proto-setter", make, (o) => {
    Object.setPrototypeOf(o, {
      set a(v: number) {
        pseen += v;
      },
      get a() {
        return "PG";
      },
    });
    delete o.a;
  });
  out.push(name + " proto setter seen " + pseen);
  both(name + " deleted-proto-readonly", make, (o) => {
    Object.setPrototypeOf(o, Object.freeze({ a: "ro" }));
    delete o.a;
  });
  both(name + " own-over-proto-setter", make, (o) => {
    Object.setPrototypeOf(o, {
      set a(v: number) {
        pseen += 1000;
      },
    });
  });
  out.push(name + " proto setter seen " + pseen);
  // table-held key z: same attributes
  for (const [tag, prep] of [
    ["plain", (o: any) => {}],
    ["frozen", (o: any) => Object.freeze(o)],
    ["nonwritable", (o: any) => Object.defineProperty(o, "z", { writable: false })],
    ["deleted-noext", (o: any) => { delete o.z; Object.preventExtensions(o); }],
    ["string", (o: any) => { o.z = "s"; }],
  ] as [string, (o: any) => void][]) {
    const o = make(3);
    prep(o);
    out.push(name + " z " + tag + " " + wrz(o, 300) + " " + desc(o, "z") + " " + String(o.z));
    out.push(name + " z " + tag + " obj " + wrz(o, { k: 1 }).slice(0, 5) + " " + typeof o.z);
  }
  // a prototype with a table: its store is seen by the objects inheriting from it
  {
    const proto = make(4);
    const kid = Object.create(proto);
    out.push(name + " inherit " + rd(kid) + " " + wr(proto, 41) + " " + rd(kid) + " " + wrs(proto, 42) + " " + rd(kid));
    Object.freeze(proto);
    out.push(name + " inherit frozen " + wr(proto, 43) + " " + wrs(proto, 44) + " " + rd(kid) + " " + wrs(kid, 45) + " " + rd(proto));
  }
  // the same site across a mixed pool, state changing under it
  {
    const pool: any[] = [];
    for (let i = 0; i < 16; i++) pool.push(make(i));
    let s = 0;
    let threw = 0;
    for (let n = 0; n < 4000; n++) {
      const o = pool[n & 15];
      const k = n & 15;
      if (n >> 4 === 62) {
        if (k % 4 === 1) Object.freeze(o);
        if (k % 4 === 2) Object.defineProperty(o, "a", { writable: false });
        if (k % 4 === 3) delete o.a;
      }
      if (n >> 4 === 150 && k % 8 === 3) Object.preventExtensions(o);
      const r = n & 16 ? wr(o, n) : wrs(o, n);
      if (r.charCodeAt(0) === 116) threw++;
      const v = rd(o);
      s += typeof v === "number" ? v : 1;
      s += rdb(o) === undefined ? 0 : 1;
    }
    out.push(name + " pool " + s + " " + threw);
  }
}
for (const line of out) console.log(line);
