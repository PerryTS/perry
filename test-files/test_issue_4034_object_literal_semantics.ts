// Refs #4034 / #3558 / #3559 / #3561: object literals must keep ordinary
// Object.prototype behavior while preserving special __proto__ and computed-key
// ordering semantics.

function assert(condition: boolean, message: string) {
  if (!condition) {
    throw new Error(message);
  }
}

assert(typeof ({} as any).toString === "function", "empty literal inherits Object.prototype");
assert(
  typeof ({ x: 1 } as any).toString === "function",
  "non-empty literal inherits Object.prototype",
);
assert(
  Object.getPrototypeOf({ x: 1 }) === Object.prototype,
  "non-empty literal prototype is Object.prototype",
);

const proto = { inherited: 7 };
const withProto = { __proto__: proto, own: 1 } as any;
assert(withProto.inherited === 7, "__proto__ object value sets literal prototype");
assert(Object.getPrototypeOf(withProto) === proto, "__proto__ object value is visible via getPrototypeOf");

const nullProto = { __proto__: null, own: 2 } as any;
assert(Object.getPrototypeOf(nullProto) === null, "__proto__ null value sets null prototype");

const ignoredProto = { __proto__: 123, own: 3 } as any;
assert(
  Object.getPrototypeOf(ignoredProto) === Object.prototype,
  "__proto__ primitive value is ignored",
);

const symbolProto = { __proto__: Symbol("p"), own: 4 } as any;
assert(
  Object.getPrototypeOf(symbolProto) === Object.prototype,
  "__proto__ symbol value is ignored",
);

const computedProto = { ["__proto__"]: 5 } as any;
assert(
  Object.getPrototypeOf(computedProto) === Object.prototype,
  "computed __proto__ is an own property, not a prototype setter",
);
assert(computedProto.__proto__ === 5, "computed __proto__ stores own value");

const orderState = { value: "bad" };
const keyObject = {
  toString() {
    orderState.value = "ok";
    return "dynamic";
  },
};
const ordered = { [keyObject as any]: orderState.value } as any;
assert(ordered.dynamic === "ok", "ToPropertyKey happens before value evaluation");

let accessorThrew = false;
try {
  const badKey = Object.create(null);
  ({ get [badKey as any]() { return 1; } });
} catch (_err) {
  accessorThrew = true;
}
assert(accessorThrew, "computed accessor key conversion can throw");

const sym = Symbol("object-literal-key");
const withSymbol = { [sym]: 9 } as any;
assert(withSymbol[sym] === 9, "computed symbol keys remain symbols");

const escapedName = { i\u0066: 42 } as any;
assert(escapedName["if"] === 42, "escaped reserved-word property names parse");
