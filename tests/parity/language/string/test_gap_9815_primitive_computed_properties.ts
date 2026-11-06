// Dynamic value reads must return the original prototype method (#9815).
function typed(s: string, key: any): any { return s[key]; }
function unknown(s: any, key: any): any { return s[key]; }
function named(s: any, key: string): any { return s[key]; }

for (const s of ["abcde", " abcdef ", "a" + "bc"]) {
  for (const key of ["charAt", "trim", "toUpperCase", "toString", "constructor"]) {
    const expected = String.prototype[key];
    console.log(key, typeof typed(s, key), typed(s, key) === expected,
      unknown(s, key) === expected, named(s, key) === expected);
  }
  const charAt = typed(s, "charAt");
  const trim = unknown(s, "trim");
  const upper = named(s, "toUpperCase");
  console.log("borrowed", charAt.call(s, 1), trim.apply(s, []), upper.bind(s)());
  console.log("length", typed(s, "length"), unknown(s, "length"), named(s, "length"));
  for (const key of [0, -0, 1, "1", -1, 1.5, 99, NaN, Infinity, "01", "1.0", "missing"]) {
    console.log("index", String(key), typed(s, key), unknown(s, key));
  }
}

const proto: any = String.prototype;
const sym = Symbol("computed");
proto[sym] = 73;
proto.custom9815 = function () { "use strict"; return this; };
proto["01"] = 81;
proto["99"] = 99;
proto["1"] = "shadowed";
Object.defineProperty(proto, "get9815", {
  configurable: true,
  get: function () { "use strict"; return typeof this + ":" + this; }
});
Object.defineProperty(Object.prototype, "inherited9815", {
  configurable: true,
  get: function () { "use strict"; return typeof this + ":" + this; }
});

for (const key of ["custom9815", "get9815", "inherited9815", "01", "99", "1", sym]) {
  const value = typed("abc", key);
  console.log("custom", typeof value, value === unknown("abc", key));
  if (typeof value === "function") console.log("receiver", value.call("xyz"));
  else console.log("value", value);
}
let coercions = 0;
const indexKey = { toString() { coercions++; return "1"; } };
const nameKey = { [Symbol.toPrimitive](hint) { coercions++; return "get9815"; } };
const symbolKey = { [Symbol.toPrimitive](hint) { coercions++; return sym; } };
console.log("coercion", typed("abc", indexKey), unknown("abc", nameKey), typed("abc", symbolKey), coercions);
console.log("bigint index", typed("abc", 1n));
console.log("symbol identity", typed("abc", Symbol.iterator) === String.prototype[Symbol.iterator]);
const boxed: any = new String("abc");
Object.setPrototypeOf(boxed, { custom9815: 42, "1": "shadowed" });
console.log("boxed", unknown(boxed, "custom9815"), unknown(boxed, "1"), unknown(boxed, "length"));
const savedConstructor = proto.constructor;
proto.constructor = 91;
console.log("constructor override", typed("abcdef", "constructor"), named("abcdef", "constructor"));
proto.constructor = savedConstructor;
const callKey = "charAt";
console.log("direct call", "abc"[callKey](1), "abc"["charAt"](1));
delete proto[sym];
delete proto.custom9815;
delete proto["01"];
delete proto["99"];
delete proto["1"];
delete proto.get9815;
delete Object.prototype.inherited9815;
console.log("deleted", typed("abc", "custom9815"), typed("abc", "99"));
