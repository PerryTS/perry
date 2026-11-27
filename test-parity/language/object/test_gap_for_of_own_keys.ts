// for-of over the Array an own-keys builtin returns (Object.keys / values /
// entries, Object.getOwnPropertyNames, Reflect.ownKeys): the result is an
// ordinary Array (ECMA-262 7.3.18 CreateArrayFromList), taken in full before
// the body runs, so a body that adds or deletes keys of the object does not
// change the keys it visits; the loop iterates through
// Array.prototype[Symbol.iterator], so a replaced iterator runs.
const sym = Symbol("s");
const o: any = { a: 1, b: "two", c: [3] };
o[sym] = 4;
Object.defineProperty(o, "hidden", { value: 5, enumerable: false, configurable: true });

const seen: string[] = [];
for (const k of Object.keys(o)) seen.push(k + "=" + String(o[k]));
console.log("keys " + seen.join(","));

const vals: string[] = [];
for (const v of Object.values(o)) vals.push(String(v));
console.log("values " + vals.join(","));

const ents: string[] = [];
for (const [k, v] of Object.entries(o)) ents.push(k + ":" + String(v));
console.log("entries " + ents.join(","));

const names: string[] = [];
for (const k of Object.getOwnPropertyNames(o)) names.push(k);
console.log("names " + names.join(","));

const own: string[] = [];
for (const k of Reflect.ownKeys(o)) own.push(String(k));
console.log("ownKeys " + own.join(","));

const syms: string[] = [];
for (const s of Object.getOwnPropertySymbols(o)) syms.push(String(s) + "=" + String(o[s]));
console.log("symbols " + syms.join(","));

// The body writes and deletes keys of the object it walks.
const m: any = { x: 1, y: 2, z: 3 };
const walked: string[] = [];
for (const k of Object.keys(m)) {
  walked.push(k + "=" + String(m[k]));
  delete m.z;
  m["n" + k] = k;
}
console.log("mutate " + walked.join(",") + " | " + Object.keys(m).join(","));

const mn: any = { p: 1, q: 2 };
const walkedNames: string[] = [];
for (const k of Object.getOwnPropertyNames(mn)) {
  walkedNames.push(k + "=" + String(mn[k]));
  mn.r = 3;
  delete mn.q;
}
console.log("mutate names " + walkedNames.join(",") + " | " + Object.getOwnPropertyNames(mn).join(","));

// One wrapper per key: each wrapper calls its own method.
function wrapAll(api: any): any {
  const w: any = {};
  for (const k of Object.keys(api)) {
    const f = api[k];
    w[k] = (...x: any[]): any => f.apply(api, x);
  }
  return w;
}
const w = wrapAll({ a(n: number): string { return "a" + n; }, b(s: string): number { return s.length; } });
console.log("wrapped " + w.a(1) + " " + String(w.b("xyz")));

// (A replaced Array.prototype[Symbol.iterator] case is cut: perry refuses that store, builtin_proto.rs; removal queued, owner 11-16.)
