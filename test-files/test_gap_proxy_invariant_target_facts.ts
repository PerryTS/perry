// Proxy trap invariants read the target own property facts: each check must
// pass or throw exactly as the spec says for data, accessor, absent and
// exotic (nested Proxy, String object, Array length) target properties.
function attempt(label: string, f: () => unknown): void {
  try { console.log(label, "ok", String(f())); } catch (e) { console.log(label, (e as Error).constructor.name); }
}
const lie = { get: () => "lie", set: () => true, has: () => false, deleteProperty: () => true };
const target: any = { plain: 1 };
Object.defineProperty(target, "fixed", { value: 2, writable: false, configurable: false });
Object.defineProperty(target, "fixedW", { value: 3, writable: true, configurable: false });
Object.defineProperty(target, "noGet", { set(_v: unknown) {}, configurable: false });
Object.defineProperty(target, "withGet", { get() { return 4; }, configurable: false });
Object.defineProperty(target, "noSet", { get() { return 5; }, configurable: false });
const p: any = new Proxy(target, lie);
attempt("get plain", () => p.plain);
attempt("get fixed", () => p.fixed);
attempt("get fixedW", () => p.fixedW);
attempt("get noGet", () => p.noGet);
attempt("get withGet", () => p.withGet);
attempt("get absent", () => p.absent);
const honest: any = new Proxy(target, { get: (t, k) => t[k] });
attempt("honest fixed", () => honest.fixed);
attempt("honest noGet", () => honest.noGet);
const undef: any = new Proxy(target, { get: () => undefined });
attempt("undef noGet", () => undef.noGet);
attempt("set fixed same", () => Reflect.set(new Proxy(target, { set: () => true }), "fixed", 2));
attempt("set fixed other", () => Reflect.set(p, "fixed", 9));
attempt("set fixedW", () => Reflect.set(p, "fixedW", 9));
attempt("set noSet", () => Reflect.set(p, "noSet", 9));
attempt("set noGet", () => Reflect.set(p, "noGet", 9));
attempt("has fixed", () => "fixed" in p);
attempt("has plain", () => "plain" in p);
attempt("delete fixed", () => delete p.fixed);
attempt("delete plain", () => delete p.plain);
const sealed: any = Object.preventExtensions({ a: 1 });
attempt("has non-extensible", () => "a" in new Proxy(sealed, lie));
attempt("delete non-extensible", () => delete new Proxy(sealed, lie).a);
const frozen = Object.freeze({ f: 7 });
attempt("get frozen", () => (new Proxy(frozen, lie) as any).f);
attempt("get frozen honest", () => (new Proxy(frozen, {}) as any).f);
const arr: any[] = [1, 2, 3];
attempt("array length lie", () => (new Proxy(arr, lie) as any).length);
attempt("array index lie", () => (new Proxy(arr, lie) as any)[0]);
Object.freeze(arr);
attempt("frozen array index lie", () => (new Proxy(arr, lie) as any)[0]);
const str: any = new String("ab");
attempt("string object index lie", () => new Proxy(str, lie)[0]);
attempt("string object length lie", () => new Proxy(str, lie).length);
attempt("string object length honest", () => new Proxy(str, {}).length);
const inner: any = new Proxy(target, {});
attempt("nested fixed lie", () => (new Proxy(inner, lie) as any).fixed);
attempt("nested plain lie", () => (new Proxy(inner, lie) as any).plain);
function fn() {}
attempt("function prototype lie", () => (new Proxy(fn, lie) as any).prototype);
attempt("function name lie", () => (new Proxy(fn, lie) as any).name);
class K { static s = 1; m() {} }
Object.defineProperty(K, "locked", { value: 8, configurable: false, writable: false });
attempt("class locked lie", () => (new Proxy(K, lie) as any).locked);
attempt("class s lie", () => (new Proxy(K, lie) as any).s);
const sym = Symbol("k");
const st: any = {};
Object.defineProperty(st, sym, { value: 1, configurable: false, writable: false });
attempt("symbol fixed lie", () => (new Proxy(st, lie) as any)[sym]);
// a store-like trap: the common case runs the check on configurable data
const raw: any = { a: 1, b: { c: 2 } };
const store: any = new Proxy(raw, { get(t, k, r) { const v = Reflect.get(t, k, r); return v; } });
let sum = 0;
for (let i = 0; i < 2000; i++) sum += store.a + store.b.c;
console.log("store", sum);
