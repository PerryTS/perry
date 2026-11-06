// Charter step 3: property attributes are facts of the SHAPE.
//
// Every row here is a place where a shape that forgot an attribute, or a cache
// that trusted a shape without asking what it says about the key, returns a
// WRONG VALUE rather than a slow one. Hot loops prime the read/write inline
// caches first, then the attribute changes, then the same sites run again.
// Output must be byte-identical to node.

function out(label: string, v: unknown): void {
  console.log(label + ": " + (typeof v === "string" ? v : JSON.stringify(v)));
}

function tryRun(label: string, f: () => unknown): void {
  try {
    out(label, f());
  } catch (e) {
    out(label, "threw " + (e as Error).constructor.name);
  }
}

// --- 1. one non-writable key must not slow or break the object's other keys (#10871)
function readA(o: any): number {
  return o.a;
}
function writeA(o: any, v: number): void {
  o.a = v;
}
{
  const O: any = { a: 1, b: 2, c: 3, d: 4 };
  let h = 0;
  for (let k = 0; k < 2000; k++) {
    writeA(O, k);
    h += readA(O);
  }
  Object.defineProperty(O, "z", { value: 7, writable: false, enumerable: false, configurable: false });
  for (let k = 0; k < 2000; k++) {
    writeA(O, k);
    h += readA(O);
  }
  out("1.sum", h);
  tryRun("1.z-write-strict", () => {
    O.z = 9;
    return O.z;
  });
  out("1.z", O.z);
  out("1.keys", Object.keys(O));
  out("1.json", JSON.stringify(O));
  out("1.desc-z", Object.getOwnPropertyDescriptor(O, "z"));
  out("1.desc-a", Object.getOwnPropertyDescriptor(O, "a"));
}

// --- 2. a primed store site meets a receiver whose key turned non-writable
{
  const objs: any[] = [];
  for (let i = 0; i < 8; i++) objs.push({ p: i, q: i * 2 });
  function storeP(o: any, v: number): void {
    o.p = v;
  }
  for (let r = 0; r < 500; r++) for (const o of objs) storeP(o, r);
  Object.defineProperty(objs[3], "p", { writable: false });
  const results: string[] = [];
  for (let i = 0; i < objs.length; i++) {
    try {
      storeP(objs[i], 1000 + i);
      results.push(String(objs[i].p));
    } catch (e) {
      results.push("threw:" + objs[i].p);
    }
  }
  out("2.stores", results.join(","));
}

// --- 3. a primed read site meets a receiver whose key became an accessor
{
  const objs: any[] = [];
  for (let i = 0; i < 6; i++) objs.push({ v: i, w: 0 });
  function readV(o: any): number {
    return o.v;
  }
  let s = 0;
  for (let r = 0; r < 500; r++) for (const o of objs) s += readV(o);
  let calls = 0;
  Object.defineProperty(objs[2], "v", {
    get() {
      calls++;
      return 100;
    },
    configurable: true,
    enumerable: true,
  });
  let t = 0;
  for (let r = 0; r < 10; r++) for (const o of objs) t += readV(o);
  out("3.before", s);
  out("3.after", t);
  out("3.getter-calls", calls);
  // Replacing the getter keeps the attributes identical: the new one must run.
  Object.defineProperty(objs[2], "v", { get: () => 200, configurable: true, enumerable: true });
  out("3.replaced", readV(objs[2]));
  // Back to a data property.
  Object.defineProperty(objs[2], "v", { value: 5, writable: true, configurable: true, enumerable: true });
  out("3.data-again", readV(objs[2]));
  out("3.desc", Object.getOwnPropertyDescriptor(objs[2], "v"));
}

// --- 4. setters on one of two look-alike objects
{
  const log: number[] = [];
  const plain: any = { x: 1, y: 2 };
  const spied: any = { x: 1, y: 2 };
  function setX(o: any, v: number): void {
    o.x = v;
  }
  for (let i = 0; i < 300; i++) {
    setX(plain, i);
    setX(spied, i);
  }
  let backing = 0;
  Object.defineProperty(spied, "x", {
    get: () => backing,
    set: (v: number) => {
      log.push(v);
      backing = v * 10;
    },
    configurable: true,
    enumerable: true,
  });
  for (let i = 0; i < 3; i++) {
    setX(plain, i);
    setX(spied, i);
  }
  out("4.plain", plain.x);
  out("4.spied", spied.x);
  out("4.log", log);
}

// --- 5. enumerability reaches every enumeration path
{
  const o: any = { a: 1, b: 2, c: 3 };
  Object.defineProperty(o, "b", { enumerable: false });
  Object.defineProperty(o, "hidden", { value: 9, enumerable: false, writable: true, configurable: true });
  const forIn: string[] = [];
  for (const k in o) forIn.push(k);
  out("5.keys", Object.keys(o));
  out("5.forin", forIn);
  out("5.json", JSON.stringify(o));
  out("5.spread", { ...o });
  out("5.assign", Object.assign({}, o));
  out("5.entries", Object.entries(o));
  out("5.values", Object.values(o));
  out("5.names", Object.getOwnPropertyNames(o));
  out("5.propIsEnum", [o.propertyIsEnumerable("a"), o.propertyIsEnumerable("b"), o.propertyIsEnumerable("hidden")]);
  out("5.descs", Object.getOwnPropertyDescriptors(o));
}

// --- 6. freeze / seal / preventExtensions on primed sites
{
  const o: any = { n: 1, m: 2 };
  function setN(t: any, v: number): void {
    t.n = v;
  }
  function addK(t: any, i: number): void {
    t["k" + (i % 3)] = i;
  }
  for (let i = 0; i < 300; i++) setN(o, i);
  const sealed: any = { n: 1, m: 2 };
  for (let i = 0; i < 300; i++) setN(sealed, i);
  const noext: any = { n: 1, m: 2 };
  for (let i = 0; i < 300; i++) setN(noext, i);

  Object.freeze(o);
  Object.seal(sealed);
  Object.preventExtensions(noext);
  tryRun("6.frozen-store", () => {
    setN(o, 42);
    return o.n;
  });
  tryRun("6.sealed-store", () => {
    setN(sealed, 42);
    return sealed.n;
  });
  tryRun("6.noext-store", () => {
    setN(noext, 42);
    return noext.n;
  });
  tryRun("6.frozen-add", () => {
    addK(o, 1);
    return Object.keys(o);
  });
  tryRun("6.sealed-add", () => {
    addK(sealed, 1);
    return Object.keys(sealed);
  });
  tryRun("6.noext-add", () => {
    addK(noext, 1);
    return Object.keys(noext);
  });
  tryRun("6.sealed-delete", () => delete sealed.m);
  tryRun("6.noext-delete", () => delete noext.m);
  out("6.state", [
    Object.isFrozen(o),
    Object.isSealed(o),
    Object.isExtensible(o),
    Object.isFrozen(sealed),
    Object.isSealed(sealed),
    Object.isExtensible(sealed),
    Object.isFrozen(noext),
    Object.isSealed(noext),
    Object.isExtensible(noext),
  ]);
  out("6.desc-frozen", Object.getOwnPropertyDescriptor(o, "n"));
  out("6.desc-sealed", Object.getOwnPropertyDescriptor(sealed, "n"));
  // A sealed key may still become non-writable.
  Object.defineProperty(sealed, "n", { writable: false });
  tryRun("6.sealed-then-ro", () => {
    setN(sealed, 7);
    return sealed.n;
  });
  out("6.sealed-now-frozen", Object.isFrozen(sealed));
  // An object that is non-extensible with every key made non-configurable and
  // non-writable IS frozen, however it got there.
  const manual: any = { a: 1 };
  Object.defineProperty(manual, "a", { writable: false, configurable: false });
  Object.preventExtensions(manual);
  out("6.manual-frozen", [Object.isFrozen(manual), Object.isSealed(manual)]);
  const empty: any = {};
  Object.preventExtensions(empty);
  out("6.empty-frozen", Object.isFrozen(empty));
}

// --- 7. attributes do not survive a delete + re-add
{
  const o: any = { a: 1, b: 2 };
  Object.defineProperty(o, "a", { value: 5, writable: false, enumerable: false, configurable: true });
  delete o.a;
  o.a = 6;
  o.a = 7;
  out("7.readded", o.a);
  out("7.desc", Object.getOwnPropertyDescriptor(o, "a"));
  out("7.keys", Object.keys(o));
}

// --- 8. attributes ride key additions (the successor shape keeps them)
{
  const o: any = { a: 1 };
  Object.defineProperty(o, "a", { writable: false });
  for (let i = 0; i < 20; i++) o["x" + i] = i;
  tryRun("8.after-adds", () => {
    o.a = 99;
    return o.a;
  });
  out("8.desc", Object.getOwnPropertyDescriptor(o, "a"));
  // ... and a delete of another key.
  delete o.x3;
  tryRun("8.after-delete", () => {
    o.a = 98;
    return o.a;
  });
}

// --- 9. two receivers reach the same attributes by different paths
{
  const p: any = { a: 1, b: 2 };
  const q: any = { a: 1, b: 2 };
  Object.defineProperty(p, "a", { enumerable: false });
  Object.defineProperty(p, "b", { writable: false });
  Object.defineProperty(q, "b", { writable: false });
  Object.defineProperty(q, "a", { enumerable: false });
  function probe(o: any): string {
    const r: string[] = [];
    try {
      o.b = 10;
      r.push("b=" + o.b);
    } catch (e) {
      r.push("b-threw");
    }
    r.push(JSON.stringify(Object.keys(o)));
    return r.join(" ");
  }
  out("9.p", probe(p));
  out("9.q", probe(q));
}

// --- 10. zod's pattern: one non-enumerable key, then many ordinary assignments
{
  class Inst {
    [k: string]: any;
    constructor(tag: number) {
      Object.defineProperty(this, "_zod", { value: { tag }, enumerable: false, configurable: true, writable: true });
      for (let i = 0; i < 40; i++) this["p" + i] = i + tag;
    }
  }
  let s = 0;
  const all: Inst[] = [];
  for (let n = 0; n < 50; n++) all.push(new Inst(n));
  function readP17(o: any): number {
    return o.p17;
  }
  for (const o of all) s += readP17(o) + o._zod.tag;
  for (const o of all) o.p17 = 1;
  for (const o of all) s += readP17(o);
  out("10.sum", s);
  out("10.keys", Object.keys(all[7]).length);
  out("10.json-has-zod", JSON.stringify(all[7]).includes("_zod"));
}

// --- 11. inherited non-writable and inherited setter
{
  const proto: any = {};
  Object.defineProperty(proto, "ro", { value: 1, writable: false, configurable: true, enumerable: true });
  let seen = -1;
  Object.defineProperty(proto, "st", {
    set(v: number) {
      seen = v;
    },
    get() {
      return seen;
    },
    configurable: true,
  });
  const child: any = Object.create(proto);
  child.other = 1;
  tryRun("11.inherited-ro", () => {
    child.ro = 5;
    return Object.prototype.hasOwnProperty.call(child, "ro");
  });
  child.st = 33;
  out("11.inherited-setter", [seen, Object.prototype.hasOwnProperty.call(child, "st")]);
}

// --- 12. symbol-keyed attributes
{
  const s = Symbol("s");
  const o: any = { a: 1 };
  Object.defineProperty(o, s, { value: 3, writable: false, enumerable: false });
  tryRun("12.sym-store", () => {
    o[s] = 4;
    return o[s];
  });
  out("12.sym-desc", String(Object.getOwnPropertyDescriptor(o, s)!.writable));
  out("12.sym-listed", Object.getOwnPropertySymbols(o).length);
}

// --- 13. getters and setters declared in an object literal
{
  let n = 0;
  const lit: any = {
    base: 2,
    get twice() {
      return this.base * 2;
    },
    set twice(v: number) {
      n = v;
    },
  };
  function readTwice(o: any): number {
    return o.twice;
  }
  let s = 0;
  for (let i = 0; i < 200; i++) {
    lit.base = i;
    s += readTwice(lit);
  }
  lit.twice = 9;
  out("13.sum", s);
  out("13.set", n);
  out("13.desc-kind", typeof Object.getOwnPropertyDescriptor(lit, "twice")!.get);
  out("13.keys", Object.keys(lit));
}
