// Builtin prototypes own exactly node's keys: no prototype other than
// Object.prototype owns an Object.prototype member node does not list as its
// own (valueOf, toLocaleString, hasOwnProperty, isPrototypeOf, ...). Those are
// reached through the [[Prototype]] chain.
const G = globalThis as any;
const OP = Object.prototype as any;
const OBJECT_MEMBERS = Object.getOwnPropertyNames(OP).sort();
console.log("Object.prototype: " + OBJECT_MEMBERS.join(","));

function ownObjectMembers(p: any): string {
  return Object.getOwnPropertyNames(p)
    .filter((k) => OBJECT_MEMBERS.indexOf(k) >= 0 && k !== "constructor")
    .sort()
    .join(",");
}

// Every global constructor's prototype: the Object.prototype members it owns.
const ctors = Object.getOwnPropertyNames(G)
  .filter((n) => {
    const v = G[n];
    return typeof v === "function" && /^[A-Z]/.test(n) && v !== Object && v.prototype && typeof v.prototype === "object";
  })
  .sort();
const owners: string[] = [];
for (const n of ctors) {
  const own = ownObjectMembers(G[n].prototype);
  if (own) owners.push(n + "[" + own + "]");
}
console.log("owners: " + owners.join(" "));

// Full own keys of the core prototypes, and of instance prototypes.
function full(label: string, p: any) {
  console.log(label + ": " + Object.getOwnPropertyNames(p).sort().join(","));
}
const core = [
  "Error", "TypeError", "RangeError", "SyntaxError", "ReferenceError", "EvalError", "URIError", "AggregateError",
  "WeakSet", "WeakRef", "FinalizationRegistry", "Promise", "Boolean", "Number", "Array",
];
for (const n of core) full(n + ".prototype", G[n].prototype);
const instances: [string, any][] = [
  ["new TypeError", new TypeError("x")],
  ["new Error", new Error("x")],
  ["new Map", new Map()],
  ["new Set", new Set()],
  ["Promise.resolve", Promise.resolve(1)],
  ["/re/", /re/],
  ["new Date", new Date(0)],
  ["new Uint8Array", new Uint8Array(1)],
  ["[]", []],
];
for (const [label, v] of instances) {
  const p = Object.getPrototypeOf(v);
  console.log(label + " proto owns: [" + ownObjectMembers(p) + "]");
}
const typedProto = Object.getPrototypeOf(Uint8Array.prototype);
console.log("%TypedArray%.prototype owns: [" + ownObjectMembers(typedProto) + "]");

// Behaviour: lookups reach Object.prototype through the chain.
console.log(String(new TypeError("x")));
console.log(`${new RangeError("y")}`, new Error("z") + "", String(new AggregateError([], "w")));
for (const n of ["Error", "TypeError", "Map", "Set", "Promise", "RegExp", "Array", "WeakMap", "Headers", "Response"]) {
  const p = G[n].prototype;
  console.log(
    n,
    p.valueOf === OP.valueOf,
    p.hasOwnProperty === OP.hasOwnProperty,
    p.isPrototypeOf === OP.isPrototypeOf,
    p.propertyIsEnumerable === OP.propertyIsEnumerable,
    p.hasOwnProperty("valueOf"),
    p.hasOwnProperty("constructor"),
  );
}
console.log("Error toLocaleString same:", Error.prototype.toLocaleString === OP.toLocaleString,
  "Map:", (Map.prototype as any).toLocaleString === OP.toLocaleString,
  "Boolean:", (Boolean.prototype as any).toLocaleString === OP.toLocaleString,
  "Array own:", Array.prototype.hasOwnProperty("toLocaleString"));
console.log("TypeError toString inherited:", TypeError.prototype.toString === Error.prototype.toString,
  TypeError.prototype.hasOwnProperty("toString"), Error.prototype.hasOwnProperty("toString"));
const e = new TypeError("q");
console.log(e.valueOf() === e, e.hasOwnProperty("message"), TypeError.prototype.hasOwnProperty("name"));
console.log((new Map([[1, 2]]) as any).valueOf() instanceof Map, (new Set() as any).toLocaleString());
console.log(OP.valueOf.call(e) === e, OP.toLocaleString.call(new Map()), OP.toLocaleString.call([1, 2]));
console.log(e.toLocaleString(), new Error("b").toLocaleString(), OP.toLocaleString.call(new RangeError("r")));
const seen: string[] = [];
for (const k in TypeError.prototype) seen.push(k);
for (const k in Map.prototype) seen.push(k);
console.log("for-in:", JSON.stringify(seen));
