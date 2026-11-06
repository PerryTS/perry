"use strict";
function get(o: any, k: string) { return o[k]; }
function put(o: any, k: string, v: any) { o[k] = v; }
function named(o: any) { return [o.x, o.missing, "x" in o, Object.hasOwn(o, "x")]; }

const proto = { x: 7, method() { return this.x; } };
const o: any = Object.create(proto);
console.log(named(o).join(","), get(o, "method").call(o));
put(o, "x", undefined);
console.log(named(o).join(","));
delete o.x;
console.log(named(o).join(","));
put(o, "x", 11);
console.log(named(o).join(","), get(o, "x"));

let calls = 0;
const accessor = {
  get x() { return this.value; },
  set x(v: number) { calls++; this.value = v; },
};
const a: any = Object.create(accessor);
a.value = 2;
put(a, "x", 13);
console.log(get(a, "x"), a.value, calls, Object.hasOwn(a, "x"));
const readonly: any = Object.create(Object.defineProperty({}, "x", { value: 17 }));
try { put(readonly, "x", 18); } catch (e: any) { console.log(e.name); }
console.log(get(readonly, "x"), Object.hasOwn(readonly, "x"));
const frozen: any = Object.freeze({ x: 19 });
try { put(frozen, "x", 20); } catch (e: any) { console.log(e.name); }
console.log(get(frozen, "x"));
const sealed: any = Object.seal({ x: 21 });
put(sealed, "x", 22);
console.log(get(sealed, "x"));

const arr: any = [3, 4]; arr.x = 23;
const typed: any = new Uint8Array([5, 6]); typed.x = 24;
console.log(get(arr, "length"), get(arr, "0"), get(arr, "x"));
console.log(get(typed, "length"), get(typed, "0"), get(typed, "x"));
const proxied: any = new Proxy({ x: 25 }, {
  get(target, key, receiver) { return Reflect.get(target, key, receiver); },
  has(target, key) { return Reflect.has(target, key); },
});
console.log(named(proxied).join(","), get(proxied, "x"));
console.log(get("abc", "length"), get("abc", "1"));
try { get(null, "x"); } catch (e: any) { console.log(e.name); }
