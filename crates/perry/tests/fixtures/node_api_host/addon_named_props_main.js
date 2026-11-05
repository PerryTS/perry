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
