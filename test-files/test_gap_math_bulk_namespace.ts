// Use the namespace as a value so reads and calls exercise its actual slots.
const math: any = Math;
const keys = Object.getOwnPropertyNames(math);
const methods = ["abs", "acos", "acosh", "asin", "asinh", "atan", "atanh",
  "atan2", "ceil", "cbrt", "expm1", "clz32", "cos", "cosh", "exp", "floor",
  "fround", "hypot", "imul", "log", "log1p", "log2", "log10", "max", "min",
  "pow", "random", "round", "sign", "sin", "sinh", "sqrt", "tan", "tanh",
  "trunc", "f16round"];
const constants = ["E", "LN10", "LN2", "LOG10E", "LOG2E", "PI", "SQRT1_2", "SQRT2"];
function check(ok: boolean, what: string) {
  if (!ok) throw new Error(what);
}
for (const name of methods) {
  const d = Object.getOwnPropertyDescriptor(math, name)!;
  check(keys.includes(name), "missing " + name);
  check(d.writable === true && d.enumerable === false && d.configurable === true, name);
  const f = d.value;
  check(typeof f === "function" && f.name === name, "function " + name);
  const length = name === "random" ? 0 :
    ["atan2", "hypot", "imul", "max", "min", "pow"].includes(name) ? 2 : 1;
  check(f.length === length, "length " + name);
  check(!Object.prototype.hasOwnProperty.call(f, "prototype"), "prototype " + name);
}
for (const name of constants) {
  const d = Object.getOwnPropertyDescriptor(math, name)!;
  check(d.writable === false && d.enumerable === false && d.configurable === false, name);
  check(typeof d.value === "number", "constant " + name);
}
const tag = Object.getOwnPropertyDescriptor(math, Symbol.toStringTag)!;
check(tag.value === "Math" && tag.writable === false && tag.enumerable === false && tag.configurable === true, "tag");
check(Object.getPrototypeOf(math) === Object.prototype, "parent");
check(Object.keys(math).length === 0, "enumeration");
check(Object.prototype.toString.call(math) === "[object Math]", "toString");
const abs = math["abs"];
const max = math["max"];
const hypot = math["hypot"];
check(abs(-7) === 7 && max(1, 9, 4) === 9 && hypot(3, 4) === 5, "detached calls");
let rejected = false;
try { new abs(2); } catch (e) { rejected = e instanceof TypeError; }
check(rejected, "non-constructor");
const child = Object.create(math);
check(child.abs === abs, "inherited original");
math.abs = function replacement(n: number) { return n + 10; };
check(child.abs(2) === 12 && abs(-2) === 2, "override and captured value");
check(Reflect.deleteProperty(math, "abs"), "delete");
check(child.abs === undefined, "inherited delete");
Object.defineProperty(math, "abs", { get() { return abs; }, configurable: true });
check(child.abs(-3) === 3, "accessor redefine");
Object.defineProperty(math, "abs", { value: abs, writable: true, configurable: true });
check(!Reflect.deleteProperty(math, "PI") && !Reflect.set(math, "PI", 0), "immutable constant");
check(Reflect.deleteProperty(math, Symbol.toStringTag), "delete tag");
Object.defineProperty(math, Symbol.toStringTag, { value: "Math", configurable: true });
Object.freeze(math);
check(Object.isFrozen(math) && !Reflect.set(math, "abs", 0), "freeze");
console.log("Math descriptors, values, mutation and inheritance OK");
