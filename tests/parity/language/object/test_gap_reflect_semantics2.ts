// Reflect non-proxy semantics, batch 2:
//   #2766 Reflect.get honors receiver (accessor `this`) + non-object TypeError
//   #2767 Reflect.apply uses thisArg + array-like argumentsList + TypeErrors
//   #2764 Reflect.has uses [[HasProperty]] (prototype chain) + non-object TypeError
//   #2763 Reflect.ownKeys includes symbol keys + non-object TypeError
//   #2761 Reflect.setPrototypeOf reports rejected changes (false) + bad-arg TypeError
//
// Proxy trap dispatch for these operations is out of scope (Perry lacks
// three-argument get traps / ownKeys traps / setPrototypeOf traps), so this
// test asserts only the ordinary-object behavior implemented here.

// --- #2766 Reflect.get receiver + accessor getter ---
const base = { marker: "base", get value() { return this.marker; } };
const recv = { marker: "recvval" };
console.log("get1:", Reflect.get(base, "value", recv)); // receiver's marker
console.log("get2:", Reflect.get({ x: 1 }, "x"));
console.log("get3:", Reflect.get({}, "missing"));
try {
  Reflect.get(1 as any, "x");
  console.log("get4: noThrow");
} catch (e) {
  console.log("get4:", e instanceof TypeError);
}

// --- #2767 Reflect.apply ---
function withThis(a: any, b: any) {
  return [(this as any) && (this as any).marker, a, b].join(":");
}
console.log("apply1:", Reflect.apply(withThis, { marker: "ctx" }, ["a", "b"]));
// array-like argumentsList
console.log(
  "apply2:",
  Reflect.apply(function (a: any, b: any) { return a + b; }, null, {
    0: 2,
    1: 3,
    length: 2,
  } as any),
);
try {
  Reflect.apply(1 as any, null, []);
  console.log("apply3: noThrow");
} catch (e) {
  console.log("apply3:", e instanceof TypeError);
}
try {
  Reflect.apply(function () {}, null, null as any);
  console.log("apply4: noThrow");
} catch (e) {
  console.log("apply4:", e instanceof TypeError);
}

// --- #2764 Reflect.has (prototype chain) ---
const hasProto = { inherited: 1 };
const hasObj: any = Object.create(hasProto);
hasObj.own = 2;
console.log("has1:", Reflect.has(hasObj, "own"));
console.log("has2:", Reflect.has(hasObj, "inherited"));
console.log("has3:", Reflect.has(hasObj, "missing"));
try {
  Reflect.has(1 as any, "x");
  console.log("has4: noThrow");
} catch (e) {
  console.log("has4:", e instanceof TypeError);
}

// --- #2763 Reflect.ownKeys (symbols) ---
const sym = Symbol("s");
const keysObj: any = {};
keysObj.visible = 2;
keysObj[sym] = 3;
console.log(
  "ownkeys1:",
  Reflect.ownKeys(keysObj)
    .map((k) => (typeof k === "symbol" ? String(k) : k))
    .join(","),
);
try {
  Reflect.ownKeys(1 as any);
  console.log("ownkeys2: noThrow");
} catch (e) {
  console.log("ownkeys2:", e instanceof TypeError);
}

// --- #2761 Reflect.setPrototypeOf (failure reporting) ---
const spOk: any = {};
console.log("setproto1:", Reflect.setPrototypeOf(spOk, { p: 1 }));
const spNonExt: any = {};
Object.preventExtensions(spNonExt);
console.log("setproto2:", Reflect.setPrototypeOf(spNonExt, { p: 1 }));
try {
  Reflect.setPrototypeOf(1 as any, {});
  console.log("setproto3: noThrow");
} catch (e) {
  console.log("setproto3:", e instanceof TypeError);
}
try {
  Reflect.setPrototypeOf({}, 1 as any);
  console.log("setproto4: noThrow");
} catch (e) {
  console.log("setproto4:", e instanceof TypeError);
}
