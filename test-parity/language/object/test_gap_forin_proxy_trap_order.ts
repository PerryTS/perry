// PXFORIN1: for-in over a Proxy runs exactly the traps node runs, in its
// order (14.7.5.9 EnumerateObjectProperties; V8): ownKeys, getPrototypeOf,
// then at each key's visit getOwnPropertyDescriptor (and getPrototypeOf when
// the key is no longer own). It never runs the has trap. A non-enumerable own
// key is skipped at its visit, a key deleted before its visit is skipped, an
// inherited key is visited through the prototype, and an own name (even a
// non-enumerable one) hides the same name on the prototype.
const log: string[] = [];
function H(name: string): any {
  return {
    get(t: any, k: any, r: any) { log.push(name + ":get:" + String(k)); return Reflect.get(t, k, r); },
    has(t: any, k: any) { log.push(name + ":has:" + String(k)); return Reflect.has(t, k); },
    ownKeys(t: any) { log.push(name + ":ownKeys"); return Reflect.ownKeys(t); },
    getOwnPropertyDescriptor(t: any, k: any) { log.push(name + ":gopd:" + String(k)); return Reflect.getOwnPropertyDescriptor(t, k); },
    getPrototypeOf(t: any) { log.push(name + ":getProto"); return Reflect.getPrototypeOf(t); },
    deleteProperty(t: any, k: any) { log.push(name + ":delete:" + String(k)); return Reflect.deleteProperty(t, k); },
  };
}
function flush(tag: string) { console.log(tag + " " + log.join(",")); log.length = 0; }

// The copy loop.
const fi: any = new Proxy({ a: 1, b: 2 }, H("fi"));
const c: any = {};
for (const k in fi) c[k] = fi[k];
console.log(JSON.stringify(c)); flush("copy");

// Keys only: no body read.
const ko: any = new Proxy({ x: 1, y: 2, z: 3 }, H("ko"));
const names: string[] = [];
for (const k in ko) names.push(k);
console.log(names.join()); flush("keys");

// Non-enumerable, symbol, inherited, shadowed.
const proto: any = { inh: 1, sh: 2 };
const t: any = Object.create(proto);
t.own = 1;
Object.defineProperty(t, "hidden", { value: 1, enumerable: false, configurable: true });
Object.defineProperty(t, "sh", { value: 1, enumerable: false, configurable: true });
t[Symbol("s")] = 1;
const mx: any = new Proxy(t, H("mx"));
const got: string[] = [];
for (const k in mx) got.push(k);
console.log(got.join()); flush("mixed");

// A key deleted before its visit is skipped.
const dl: any = new Proxy({ p: 1, q: 2, r: 3 }, H("dl"));
const vis: string[] = [];
for (const k in dl) {
  vis.push(k);
  if (k === "p") delete dl.q;
}
console.log(vis.join()); flush("delete");

// for-in in a function over a Proxy argument.
function count(o: any): number { let n = 0; for (const _k in o) n++; return n; }
console.log(count(new Proxy({ u: 1, v: 2 }, H("fn")))); flush("fn");
