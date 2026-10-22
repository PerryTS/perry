// Builtin methods on untyped receivers, computed-key method calls and
// run-time-built const keys (THR1). Every line must match node.
const TypeId = "~t/Type";
const args = `${TypeId}/args`;
const evaluate = `${TypeId}/evaluate`;
const idx = `${1}${2}`;
const one = `${TypeId}`;
const sum = "a" + TypeId + "/b";

function show(label: string, v: any) { console.log(label, typeof v, JSON.stringify(v)); }
function viaAny(v: any): any { return v; }

// 1. String-named builtins on untyped arrays and strings.
const arrs: any[] = [viaAny([1, 2, 3, 2]), viaAny(["a", "b", "a"]), viaAny("abcab")];
const u8: any = viaAny(new Uint8Array([5, 6, 5]));
show("u8-indexOf", u8.indexOf(6));
show("u8-includes", u8.includes(5));
show("u8-lastIndexOf", u8.lastIndexOf(5));
show("u8-at", u8.at(-1));
for (const a of arrs) {
  show("indexOf", a.indexOf(a[1]));
  show("indexOf2", a.indexOf(a[0], 1));
  show("lastIndexOf", a.lastIndexOf(a[0]));
  show("includes", a.includes(a[1]));
  show("includes-miss", a.includes(99));
  show("at", a.at(-1));
  show("slice", String(a.slice(1, 3)));
}
show("concat", viaAny([1]).concat([2], 3));
show("concat-str", viaAny("x").concat("y", 1));
// Own override and user methods of builtin names.
const own: any = viaAny([1, 2, 3]);
own.indexOf = function (this: any, x: any) { return "own:" + x + ":" + this.length; };
show("own-indexOf", own.indexOf(2));
const user: any = viaAny({ indexOf(x: any) { return "user:" + x; }, includes: (x: any) => x === 1, push(x: any) { return "upush:" + x; } });
show("user-indexOf", user.indexOf(7));
show("user-includes", user.includes(1));
show("user-push", user.push(3));
class Bag { items: any[] = []; push(x: any) { this.items.push(x); return this.items.length * 10; } indexOf(x: any) { return this.items.indexOf(x) + 100; } }
const bag: any = viaAny(new Bag());
show("class-push", bag.push("q"));
show("class-indexOf", bag.indexOf("q"));
class MyArr extends Array<any> { indexOf(x: any) { return -42; } }
const ma: any = viaAny(MyArr.from([1, 2]));
show("subclass-indexOf", ma.indexOf(1));
show("subclass-includes", ma.includes(2));
const prox: any = new Proxy([1, 2, 3], {});
show("proxy-indexOf", prox.indexOf(3));
show("proxy-includes", prox.includes(4));
// push on an untyped array.
const p: any = viaAny([]);
for (let i = 0; i < 5; i++) p.push(i * 2);
show("push", p);
show("push-multi", p.push(1, 2, 3));
try { const z: any = viaAny(Object.freeze([1])); z.push(2); } catch (e: any) { console.log("frozen-push", e instanceof TypeError); }
try { const n: any = viaAny(null); n.indexOf(1); } catch (e: any) { console.log("null-indexOf", e instanceof TypeError); }
try { const n: any = viaAny(5); n.indexOf(1); } catch (e: any) { console.log("num-indexOf", e instanceof TypeError); }

// 2. Computed-key method calls.
const Proto: any = {
  [evaluate](this: any, n: number) { return this[args] + n; },
  get [one]() { return (x: any) => "getter:" + x; },
};
const self: any = Object.create(Proto);
self[args] = 40;
show("keyed-proto", self[evaluate](2));
show("keyed-getter", self[one](1));
const obj: any = { m(this: any, x: any) { return this === obj ? "this-ok:" + x : "this-bad"; }, arrow: (x: any) => typeof x, v: 5 };
const mk = "m", ak = "arrow";
show("keyed-own", obj[mk](1));
show("keyed-arrow", obj[ak]("s"));
const sym = Symbol("s");
obj[sym] = function (this: any) { return this.v; };
show("keyed-symbol", obj[sym]());
const fns: any = { "0": function (this: any) { return this.v; }, v: 9 };
const zk = "0";
show("keyed-index-str", fns[zk]());
class K { base = 3; [evaluate](n: number) { return this.base * n; } static [args]() { return "static"; } }
const k: any = new K();
show("keyed-class", k[evaluate](4));
show("keyed-static", (K as any)[args]());
const bound: any = { f: function (this: any) { return this === undefined ? "undef" : typeof this; }.bind(undefined) };
const fk = "f";
show("keyed-bound", bound[fk]());
const dyn = "dyn" + String(Math.floor(1.5));
obj[dyn] = function (this: any) { return this.v + 1; };
show("keyed-runtime-key", obj[dyn]());
try { obj["v"](); } catch (e: any) { console.log("keyed-noncallable", e instanceof TypeError); }
try { const u: any = viaAny(undefined); u[mk](); } catch (e: any) { console.log("keyed-undefined", e instanceof TypeError); }
const s: any = viaAny("hello");
const up = "toUpperCase";
show("keyed-string", s[up]());
const arrM: any = viaAny([3, 1, 2]);
const jk = "join";
show("keyed-array-builtin", arrM[jk]("-"));

// 3. Run-time-built const keys are ordinary strings.
show("args", args);
show("args-eq", args === "~t/Type/args");
show("args-len", args.length);
show("idx", idx);
show("one", one);
show("sum", sum);
const o2: any = {};
o2[args] = 1; o2[idx] = 2; o2[sum] = 3;
show("keys", Object.keys(o2));
show("has", Object.prototype.hasOwnProperty.call(o2, "~t/Type/args"));
show("read-literal", o2["~t/Type/args"]);
const arrK: any = [0, 1, 2];
arrK[idx] = "twelve";
show("idx-elem", arrK.length);
const m = new Map<any, any>([[args, "mapped"]]);
show("map-key", m.get("~t/Type/args"));

// 4. A modified Array.prototype is honoured afterwards.
(Array.prototype as any).includes = function () { return "patched"; };
show("patched-includes", viaAny([1]).includes(1));
