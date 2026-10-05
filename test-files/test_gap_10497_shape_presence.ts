// [[HasProperty]] reads keys and prototype edges, never property values.
const o: any = { own: undefined };
const p: any = Object.prototype;
let gets = 0;
Object.defineProperty(p, "presence10497", {
  configurable: true,
  get() { gets++; return undefined; },
});
Object.defineProperty(p, "undefined10497", {
  configurable: true, writable: true, value: undefined,
});
console.log("default", "own" in o, "presence10497" in o, "undefined10497" in o, gets);
console.log("own", Object.hasOwn(o, "own"), Object.hasOwn(o, "presence10497"));
delete p.presence10497;
delete p.undefined10497;
console.log("deleted", "presence10497" in o, "undefined10497" in o, gets);

const parent: any = Object.create(null);
const child: any = Object.create(parent);
Object.defineProperty(parent, "presence10497", {
  configurable: true,
  get() { gets++; throw new Error("presence must not call a getter"); },
});
console.log("linked", "presence10497" in child, "toString" in child, gets);
delete parent.presence10497;
parent.presence10497 = undefined;
console.log("replaced", "presence10497" in child, Object.hasOwn(child, "presence10497"), gets);
Object.setPrototypeOf(child, null);
console.log("null", "presence10497" in child, "toString" in child);
Object.setPrototypeOf(child, o);
console.log("retargeted", "own" in child, "toString" in child);

let has = 0;
Object.setPrototypeOf(child, new Proxy({}, { has(_target, key) { has++; return key === "virtual"; } }));
console.log("proxy", "virtual" in child, "absent" in child, has);
let conversions = 0;
const key: any = { toString() { conversions++; return "own"; } };
console.log("key", key in o, conversions);
const sym = Symbol("present");
o[sym] = undefined;
console.log("symbol", sym in o);
class C { method() {} }
console.log("class", "method" in new C());
Object.setPrototypeOf(C.prototype, null);
console.log("class-null", "method" in C.prototype);
console.log("prototype", Object.getPrototypeOf(o) === p, Object.getPrototypeOf(p) === null);

// Inline and heap key representations compare bytes, including Unicode/NUL.
const names = ["", "a\0b", "é", "😀", "abcde", "abcdef"];
const keys: any = Object.create(null);
for (const name of names) keys[name] = undefined;
console.log("bytes", names.map(name => name in keys).join(","), "a" in keys, "abcdf" in keys);
for (const name of names) delete keys[name];
console.log("holes", names.some(name => name in keys));

// Nonconstructible builtin bodies retain explicit-this call/apply behavior.
const hop = Object.prototype.hasOwnProperty;
console.log("call", hop.call(o, "own"), hop.apply(o, ["presence10497"]));
console.log("primitive", Object.prototype.valueOf.call(3).valueOf());
const method: any = function (value: number) { return this.x + value; };
console.log("function", method.call({ x: 4 }, 2), method.apply({ x: 5 }, [3]));
const bound = method.bind({ x: 6 });
console.log("bound", bound.call({ x: 0 }, 4));
const callable = new Proxy(method, { apply(target, receiver, args) { return target.apply(receiver, args) + 1; } });
console.log("callable", callable.call({ x: 7 }, 2));

const functionProperties: any = function () {};
console.log("function-miss", functionProperties.absent10497 === undefined);
functionProperties.absent10497 = undefined;
console.log("function-own", "absent10497" in functionProperties, Object.hasOwn(functionProperties, "absent10497"));
delete functionProperties.absent10497;
console.log("function-delete", "absent10497" in functionProperties, functionProperties.absent10497 === undefined);

console.log("wrapper", "0" in new String("xy"), "length" in new String("xy"), "2" in new String("xy"));
