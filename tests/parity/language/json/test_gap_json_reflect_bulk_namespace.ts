const j: any = JSON;
const r: any = Reflect;
function check(ok: boolean, message: string) {
  if (!ok) throw new Error(message);
}
function checkNamespace(ns: any, tag: string, lengths: any) {
  const names = Object.getOwnPropertyNames(ns).sort();
  check(names.join(",") === Object.keys(lengths).sort().join(","), "names " + tag);
  check(Object.keys(ns).length === 0, "enumerable " + tag);
  check(Object.getPrototypeOf(ns) === Object.prototype, "parent " + tag);
  check(Object.prototype.toString.call(ns) === "[object " + tag + "]", "tag " + tag);
  const d = Object.getOwnPropertyDescriptor(ns, Symbol.toStringTag)!;
  check(d.value === tag && !d.writable && !d.enumerable && d.configurable, "tag descriptor");
  for (const name of names) {
    const d = Object.getOwnPropertyDescriptor(ns, name)!;
    const f = d.value;
    check(d.writable && !d.enumerable && d.configurable, "descriptor " + name);
    check(typeof f === "function" && f.name === name && f.length === lengths[name], "metadata " + name);
    check(!Object.prototype.hasOwnProperty.call(f, "prototype"), "prototype " + name);
    let rejected = false;
    try { new f(); } catch (error) { rejected = error instanceof TypeError; }
    check(rejected, "non-constructor " + name);
  }
}
checkNamespace(j, "JSON", { parse: 2, stringify: 3, rawJSON: 1, isRawJSON: 1 });
checkNamespace(r, "Reflect", {
  apply: 3, construct: 2, defineProperty: 3, deleteProperty: 2, get: 2,
  getOwnPropertyDescriptor: 2, getPrototypeOf: 1, has: 2, isExtensible: 1,
  ownKeys: 1, preventExtensions: 1, set: 3, setPrototypeOf: 2,
});
// Descriptor values force calls through the actual allocated functions.
const parse = Object.getOwnPropertyDescriptor(j, "parse")!.value;
const stringify = Object.getOwnPropertyDescriptor(j, "stringify")!.value;
const rawJSON = Object.getOwnPropertyDescriptor(j, "rawJSON")!.value;
const isRawJSON = Object.getOwnPropertyDescriptor(j, "isRawJSON")!.value;
check(parse('{"x":4}', function(key: string, value: any) { return key === "x" ? value + 1 : value; }).x === 5, "reviver");
check(stringify({ x: 5, y: 9 }, ["x"], 2) === '{\n  "x": 5\n}', "replacer/space");
const raw = rawJSON("12345678901234567890");
check(isRawJSON(raw) && !isRawJSON({}), "raw JSON detection");
check(stringify({ x: raw }) === '{"x":12345678901234567890}', "raw JSON stringify");
const get = Object.getOwnPropertyDescriptor(r, "get")!.value;
const set = Object.getOwnPropertyDescriptor(r, "set")!.value;
const apply = Object.getOwnPropertyDescriptor(r, "apply")!.value;
const construct = Object.getOwnPropertyDescriptor(r, "construct")!.value;
const target: any = { x: 3, get value() { return this.x; } };
const receiver: any = { x: 11 };
check(get(target, "value") === 3 && get(target, "value", receiver) === 11, "get receiver");
check(set(target, "x", 17, receiver) && receiver.x === 17 && target.x === 3, "set receiver");
function sum(this: any, a: number, b: number) { return this.x + a + b; }
check(apply(sum, receiver, [2, 4]) === 23, "apply receiver/arguments");
class Box { value: number; constructor(value: number) { this.value = value; } }
class Other {}
const ordinary = construct(Box, [29]);
const alternate = construct(Box, [31], Other);
check(ordinary.value === 29 && Object.getPrototypeOf(ordinary) === Box.prototype, "construct default newTarget");
check(alternate.value === 31 && Object.getPrototypeOf(alternate) === Other.prototype, "construct newTarget");
function mutate(ns: any, method: string, saved: any, tag: string) {
  const child = Object.create(ns);
  check(child[method] === saved, "inherited method");
  ns[method] = 23;
  check(child[method] === 23, "inherited replacement");
  check(Reflect.deleteProperty(ns, method) && child[method] === undefined, "delete");
  Object.defineProperty(ns, method, { get() { return saved; }, configurable: true });
  check(child[method] === saved, "accessor replacement");
  Object.defineProperty(ns, method, { value: saved, writable: true, configurable: true });
  check(Reflect.deleteProperty(ns, Symbol.toStringTag), "delete tag");
  Object.defineProperty(ns, Symbol.toStringTag, { value: tag, configurable: true });
  Object.freeze(ns);
  check(Object.isFrozen(ns) && !Reflect.set(ns, method, 0), "freeze");
}
mutate(j, "parse", parse, "JSON");
mutate(r, "get", get, "Reflect");
check(parse("7") === 7 && get(receiver, "x") === 17, "captured methods after mutation");
console.log("JSON and Reflect descriptors, detached calls, mutation and inheritance OK");
