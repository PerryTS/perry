// #11868: plain objects inherit the property on %Object.prototype%,
// independently of the writable global Object binding.
const Orig = Object;
const proto = Orig.prototype;
const descriptor = Orig.getOwnPropertyDescriptor(proto, "constructor")!;
const literal: any = { a: 1 };
const empty: any = {};
const parsed: any = JSON.parse('{"a":1}');
const created: any = Orig.create(proto);
const nullProto: any = Orig.create(null);
const own: any = { constructor: "own" };
const key = JSON.parse('"constructor"');

function check(label: string, expected: any) {
  console.log(label,
    literal.constructor === expected,
    empty.constructor === expected,
    parsed.constructor === expected,
    created.constructor === expected,
    literal[key] === expected,
    Reflect.get(literal, key) === expected);
  console.log("own/null", own.constructor, nullProto.constructor === undefined);
}

(globalThis as any).Object = function Fake() {};
console.log("issue", literal.constructor === Orig,
  literal.constructor === (globalThis as any).Object);
check("reassigned", Orig);
console.log("new literal", ({ a: 2 }).constructor === Orig);
console.log("prototype", Orig.getPrototypeOf(literal) === proto,
  proto.constructor === Orig,
  Orig.getOwnPropertyDescriptor(proto, "constructor")!.value === Orig);

delete (globalThis as any).Object;
check("global deleted", Orig);
(globalThis as any).Object = Orig;

// The prototype property itself remains writable and configurable.
(proto as any).constructor = "changed";
check("prototype changed", "changed");
(proto as any).constructor = undefined;
check("prototype undefined", undefined);
delete (proto as any).constructor;
check("prototype deleted", undefined);
console.log("absent", "constructor" in literal, "constructor" in proto);

Orig.defineProperty(proto, "constructor", {
  configurable: true,
  get() { return this; },
});
console.log("getter receiver", literal.constructor === literal,
  empty.constructor === empty, parsed.constructor === parsed,
  created.constructor === created, literal[key] === literal,
  Reflect.get(literal, key, own) === own);
Orig.defineProperty(proto, "constructor", descriptor);
check("restored", Orig);
