// Charter step 3: a key's attributes live WITH the key, in the keys array.
//
// Key lists are shared: one canonical backing serves every object whose keys
// are a prefix of it, and a dictionary-mode object keeps a private list that
// it edits in place. Each row below is a way a list that forgot to carry its
// attributes — through a fork, a copy, a delete, a latch or a rebuild — shows
// up as a WRONG VALUE on some other object or some other key. Output must be
// byte-identical to node.

function out(label: string, v: unknown): void {
  console.log(label + ": " + (typeof v === "string" ? v : JSON.stringify(v)));
}

function attempt(label: string, f: () => unknown): void {
  try {
    out(label, f());
  } catch (e) {
    out(label, "threw " + (e as Error).constructor.name);
  }
}

function summary(o: any, key: string): string {
  const d = Object.getOwnPropertyDescriptor(o, key);
  if (!d) return "absent";
  const kind = "value" in d ? "data" : "accessor";
  return [kind, d.writable === false ? "ro" : "w", d.enumerable ? "e" : "ne", d.configurable ? "c" : "nc"].join("/");
}

// --- 1. siblings on one backing: an attribute on one never reaches the other
{
  const a: any = { p: 1, q: 2, r: 3 };
  const b: any = { p: 1, q: 2, r: 3 };
  Object.defineProperty(a, "q", { writable: false });
  a.s = 4;
  b.s = 4;
  attempt("1.a-q", () => {
    a.q = 9;
    return a.q;
  });
  attempt("1.b-q", () => {
    b.q = 9;
    return b.q;
  });
  out("1.a", [summary(a, "p"), summary(a, "q"), summary(a, "r"), summary(a, "s")]);
  out("1.b", [summary(b, "p"), summary(b, "q"), summary(b, "r"), summary(b, "s")]);
}

// --- 2. the same key, added with and without attributes, at the same position
{
  const plain: any = { k0: 0 };
  const acc: any = { k0: 0 };
  plain.k1 = 1;
  Object.defineProperty(acc, "k1", { get: () => 11, enumerable: true, configurable: true });
  plain.k2 = 2;
  acc.k2 = 2;
  out("2.plain", [plain.k1, summary(plain, "k1"), Object.keys(plain)]);
  out("2.acc", [acc.k1, summary(acc, "k1"), Object.keys(acc)]);
}

// --- 3. an exports object: a getter per re-export, appended in order
{
  const exp: any = {};
  Object.defineProperty(exp, "__esModule", { value: true });
  const names: string[] = [];
  for (let i = 0; i < 300; i++) names.push("n" + i);
  for (const n of names) {
    const v = n.length;
    Object.defineProperty(exp, n, { enumerable: true, get: () => v });
  }
  let s = 0;
  for (const n of names) s += exp[n];
  out("3.sum", s);
  out("3.keys", Object.keys(exp).length);
  out("3.first", [summary(exp, "__esModule"), summary(exp, "n0"), summary(exp, "n299")]);
  attempt("3.write-getter", () => {
    exp.n5 = 1;
    return exp.n5;
  });
  // A second exports object with the same keys shares the layout and must
  // read its own closures.
  const exp2: any = {};
  Object.defineProperty(exp2, "__esModule", { value: true });
  for (const n of names) {
    const v = n.length * 2;
    Object.defineProperty(exp2, n, { enumerable: true, get: () => v });
  }
  let s2 = 0;
  for (const n of names) s2 += exp2[n];
  out("3.sum2", s2);
}

// --- 4. change a MIDDLE key: the tail is rebuilt, a sibling keeps its list
{
  const mk = () => ({ a: 1, b: 2, c: 3, d: 4, e: 5 });
  const x: any = mk();
  const y: any = mk();
  Object.defineProperty(x, "b", { enumerable: false });
  x.f = 6;
  y.f = 6;
  out("4.x", [Object.keys(x), summary(x, "b"), summary(x, "f")]);
  out("4.y", [Object.keys(y), summary(y, "b"), summary(y, "f")]);
  Object.defineProperty(x, "b", { enumerable: true });
  out("4.x-restored", [Object.keys(x), summary(x, "b")]);
}

// --- 5. delete from a list that carries attributes: positions shift, attributes follow
{
  const o: any = { a: 1, b: 2, c: 3, d: 4 };
  Object.defineProperty(o, "c", { writable: false, enumerable: false });
  Object.defineProperty(o, "d", { get: () => 44, enumerable: true, configurable: true });
  delete o.a;
  out("5.keys", Object.keys(o));
  out("5.attrs", [summary(o, "b"), summary(o, "c"), summary(o, "d")]);
  attempt("5.c-write", () => {
    o.c = 9;
    return o.c;
  });
  out("5.d", o.d);
  delete o.b;
  out("5.after-b", [Object.keys(o), summary(o, "c"), summary(o, "d"), o.d]);
  o.a = 10;
  out("5.readd", [Object.keys(o), summary(o, "a")]);
}

// --- 6. a dictionary-mode object: many unique keys, then attributes, deletes, adds
{
  const dict: any = {};
  for (let i = 0; i < 2000; i++) dict["u" + i * 7] = i;
  Object.defineProperty(dict, "u70", { writable: false, enumerable: false });
  Object.defineProperty(dict, "u140", { get: () => -1, enumerable: true, configurable: true });
  for (let i = 0; i < 100; i++) delete dict["u" + i * 14];
  for (let i = 0; i < 50; i++) dict["v" + i] = i;
  Object.defineProperty(dict, "v7", { enumerable: false, value: 77 });
  const keys = Object.keys(dict);
  out("6.count", keys.length);
  out("6.has", ["u70" in dict, "u140" in dict, "u7" in dict]);
  out("6.attrs", [summary(dict, "u7"), summary(dict, "u21"), summary(dict, "v7"), summary(dict, "v8")]);
  attempt("6.u21", () => dict.u21);
  let s = 0;
  for (const k of keys) s += dict[k];
  out("6.sum", s);
  Object.freeze(dict);
  attempt("6.frozen-write", () => {
    dict.u7 = 1;
    return dict.u7;
  });
  out("6.frozen", [Object.isFrozen(dict), summary(dict, "u7"), summary(dict, "v8")]);
}

// --- 7. freeze after tombstones and re-adds
{
  const o: any = {};
  for (let i = 0; i < 20; i++) o["t" + i] = i;
  for (let i = 0; i < 20; i += 3) delete o["t" + i];
  o.t0 = 100;
  Object.freeze(o);
  out("7.keys", Object.keys(o));
  out("7.frozen", [Object.isFrozen(o), summary(o, "t0"), summary(o, "t1")]);
  attempt("7.write", () => {
    o.t1 = 5;
    return o.t1;
  });
}

// --- 8. a primed read site over many layouts that differ only in attributes
{
  function readM(o: any): number {
    return o.m;
  }
  const objs: any[] = [];
  for (let i = 0; i < 6; i++) {
    const o: any = { l: i, m: i * 10, n: i };
    if (i % 2 === 1) {
      const v = i * 1000;
      Object.defineProperty(o, "m", { get: () => v, enumerable: true, configurable: true });
    } else if (i % 3 === 0) {
      Object.defineProperty(o, "m", { writable: false });
    }
    objs.push(o);
  }
  let s = 0;
  for (let r = 0; r < 200; r++) for (const o of objs) s += readM(o);
  out("8.sum", s);
  const w: string[] = [];
  for (const o of objs) {
    try {
      o.m = 1;
      w.push(String(o.m));
    } catch (e) {
      w.push("threw");
    }
  }
  out("8.writes", w.join(","));
}

// --- 9. class prototypes: an accessor and a read-only key on the chain
{
  class Base {
    x = 1;
  }
  Object.defineProperty(Base.prototype, "ro", { value: 5, writable: false, configurable: true });
  let seen = 0;
  Object.defineProperty(Base.prototype, "acc", {
    get() {
      return seen;
    },
    set(v: number) {
      seen = v * 2;
    },
    configurable: true,
  });
  const items: any[] = [];
  for (let i = 0; i < 5; i++) items.push(new Base());
  function setAll(v: number): string {
    const r: string[] = [];
    for (const it of items) {
      try {
        it.ro = v;
        r.push(String(Object.prototype.hasOwnProperty.call(it, "ro")));
      } catch (e) {
        r.push("threw");
      }
      it.acc = v;
    }
    return r.join(",") + " seen=" + seen;
  }
  out("9.set", setAll(3));
  delete (Base.prototype as any).ro;
  out("9.after-delete", setAll(4));
}

// --- 10. a receiver in stable-tombstone mode moves onto a shared list with
// attributes, then gains an accessor: the accessor must be reported (its
// ShapeId must not be updated in place as if the list were still private)
{
  for (const mk of [() => ({}), () => JSON.parse("{}")]) {
    const o: any = mk();
    o.a = 1;
    o.dynamic = "x";
    o.extra = 3;
    delete o.a;
    o.a = 7;
    Object.defineProperty(o, "hidden", { value: 99, enumerable: false });
    Object.defineProperty(o, "getter", { get: () => "seen", enumerable: true });
    const d: any = Object.getOwnPropertyDescriptor(o, "getter");
    out("10.desc", [typeof d.get, d.enumerable, d.configurable, o.getter]);
    out("10.json", JSON.stringify(o));
  }
}

// --- 11. replacing an accessor's function under unchanged attributes
{
  const o: any = {};
  Object.defineProperty(o, "v", { get: () => 1, configurable: true });
  function readV(x: any): number {
    return x.v;
  }
  let s = 0;
  for (let i = 0; i < 300; i++) s += readV(o);
  Object.defineProperty(o, "v", { get: () => 2, configurable: true });
  for (let i = 0; i < 300; i++) s += readV(o);
  out("11.sum", s);
}
