// Node-API named property operations follow [[Set]], [[Get]] and [[HasProperty]]
// on every receiver: a setter on the prototype runs, numeric-like names are
// index keys, a frozen object refuses, and addon-built objects read back.
const addon = require("fixture-named-props");

class Point {
  constructor() {
    this.log = [];
  }
  set x(value) {
    this.log.push("set x " + value);
  }
  get x() {
    return "getter";
  }
}
const point = new Point();
addon.setNamed(point, "x", 7);
console.log("prototype setter", JSON.stringify(point.log), Object.prototype.hasOwnProperty.call(point, "x"), addon.getNamed(point, "x"), addon.hasNamed(point, "x"));

const plain = {};
for (const name of ["b", "1", "a", "0", "01", "-1", "4294967295", "4294967294", "1.5"]) {
  addon.setNamed(plain, name, name);
}
console.log("key order", JSON.stringify(Object.keys(plain)));
console.log("numeric reads", addon.getNamed(plain, "0"), plain[0], addon.getNamed(plain, "01"), addon.hasNamed(plain, "1"), addon.hasNamed(plain, "2"));

const list = [];
addon.setNamed(list, "0", "zero");
addon.setNamed(list, "2", "two");
addon.setNamed(list, "name", "n");
console.log("array", list.length, JSON.stringify(list), list.name, addon.getNamed(list, "length"), addon.hasNamed(list, "1"), addon.hasNamed(list, "2"));
addon.setNamed(list, "length", 1);
console.log("array length store", list.length, JSON.stringify(list));

const frozen = Object.freeze({ k: 1 });
console.log("frozen", addon.setNamedStatus(frozen, "k", 2), frozen.k, addon.setNamedStatus(frozen, "z", 3), "z" in frozen);

const base = { inherited: "yes" };
const child = Object.create(base);
console.log("chain", addon.getNamed(child, "inherited"), addon.hasNamed(child, "inherited"), addon.hasNamed(child, "toString"), addon.hasNamed(child, "missing"), String(addon.getNamed(child, "missing")));

addon.setNamed(child, "inherited", "own");
console.log("shadowing store", child.inherited);

const readOnly = Object.create(Object.defineProperty({}, "ro", { value: 1, writable: false }));
addon.setNamed(readOnly, "ro", 2);
console.log("read-only on prototype", readOnly.ro, Object.keys(readOnly).length);

const viaProto = {};
addon.setNamed(viaProto, "__proto__", { fromProto: 1 });
console.log("__proto__", viaProto.fromProto, Object.keys(viaProto).length);

const unicode = {};
addon.setNamed(unicode, "ключ", 1);
console.log("utf8", JSON.stringify(Object.keys(unicode)), addon.getNamed(unicode, "ключ"), unicode["ключ"]);

const made = addon.make(1000);
let ports = 0;
for (const packet of made) ports += packet.port === 4433 ? 1 : 0;
console.log("made", made.length, JSON.stringify(made[999]), JSON.stringify(Object.keys(made[0])), ports, made[7].length);
addon.setNamed(made[1], "extra", true);
console.log("made extra", JSON.stringify(Object.keys(made[1])), addon.hasNamed(made[1], "extra"), addon.hasNamed(made[2], "extra"));

// WTF26: a named access is an unknown-receiver fixed-key site (the layout probe first, the complete
// operation on a decline); every probe decline below must still answer as node does.

const ownGetter = { get k() { return "own getter"; } };
console.log("own getter", addon.getNamed(ownGetter, "k"), addon.setNamedStatus(ownGetter, "k", 1), ownGetter.k, addon.hasNamed(ownGetter, "k"));
let captured = 5;
const bindingGetter = { get k() { return captured; } };
captured = 6;
console.log("binding getter", addon.getNamed(bindingGetter, "k"), addon.getNamed(Object.create(bindingGetter), "k"));
const ownSetter = { set k(value) { this.seen = value; } };
addon.setNamed(ownSetter, "k", 3);
console.log("own setter", ownSetter.seen, String(addon.getNamed(ownSetter, "k")), JSON.stringify(Object.keys(ownSetter)));
const throwing = { set k(value) { throw new Error("no " + value); } };
console.log("throwing setter", addon.setNamedStatus(throwing, "k", 1));

const nonWritable = Object.defineProperty({}, "ro", { value: 1, writable: false, enumerable: true });
console.log("own non-writable", addon.setNamedStatus(nonWritable, "ro", 2), nonWritable.ro);
const hidden = Object.defineProperty({}, "h", { value: 1, writable: true, enumerable: false });
addon.setNamed(hidden, "h", 2);
console.log("non-enumerable writable", hidden.h, Object.keys(hidden).length, addon.getNamed(hidden, "h"));
const sealed = Object.seal({ k: 1 });
console.log("sealed", addon.setNamedStatus(sealed, "k", 2), sealed.k, addon.setNamedStatus(sealed, "z", 3), "z" in sealed);
const closed = Object.preventExtensions({ k: 1 });
console.log("non-extensible", addon.setNamedStatus(closed, "k", 2), closed.k, addon.setNamedStatus(closed, "z", 3), "z" in closed, String(addon.getNamed(closed, "z")));

const shadowBase = { inherited: "base" };
const shadowChild = Object.create(shadowBase);
addon.setNamed(shadowChild, "inherited", "own");
console.log("shadowing store", addon.getNamed(shadowChild, "inherited"), shadowBase.inherited, Object.keys(shadowChild).length);

const rep = { n: 1, m: 2 };
const twin = { n: 1, m: 2 };
addon.setNamed(rep, "n", "text");
addon.setNamed(rep, "m", 1.5);
console.log("representation", rep.n, rep.m, twin.n, twin.m, addon.getNamed(rep, "n"), addon.getNamed(rep, "m"));

const trapLog = [];
const proxy = new Proxy({ t: 1 }, {
  get(target, key) { trapLog.push("get " + String(key)); return 42; },
  set(target, key, value) { trapLog.push("set " + String(key) + " " + value); return true; },
  has(target, key) { trapLog.push("has " + String(key)); return key === "t"; },
});
console.log("proxy", addon.getNamed(proxy, "t"), addon.setNamedStatus(proxy, "t", 9), addon.hasNamed(proxy, "zz"), JSON.stringify(trapLog));

class Fields { constructor() { this.x = 1; this.y = 2; } get sum() { return this.x + this.y; } }
const fields = new Fields();
addon.setNamed(fields, "y", 10);
console.log("class instance", addon.getNamed(fields, "x"), addon.getNamed(fields, "sum"), fields.y, addon.hasNamed(fields, "sum"));

function fn() {}
addon.setNamed(fn, "custom", 1);
console.log("function", addon.getNamed(fn, "name"), addon.getNamed(fn, "length"), fn.custom, addon.getNamed(fn, "custom"));

const bytes = new Uint8Array(4);
addon.setNamed(bytes, "1", 7);
addon.setNamed(bytes, "-0", 5);
addon.setNamed(bytes, "tag", "t");
console.log("typed array", addon.getNamed(bytes, "1"), String(addon.getNamed(bytes, "-0")), addon.hasNamed(bytes, "-0"), addon.getNamed(bytes, "length"), String(addon.getNamed(bytes, "1.5")), bytes.tag, JSON.stringify(Array.from(bytes)));

const hooks = require("async_hooks");
console.log("async_hooks", typeof addon.getNamed(hooks, "createHook"), addon.hasNamed(hooks, "createHook"));
