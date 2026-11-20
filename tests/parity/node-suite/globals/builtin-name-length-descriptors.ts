// #3143 — spec-correct .length / .name own-property descriptors for built-ins.
// verifyProperty (Test262 harness/propertyHelper.js) checks the *full* descriptor
// of every built-in method/property, so each facet below must match Node.
//
// NOTE: reading a prototype method *as a value off an instance* (`[].map`) and
// resolving its .name/.length is tracked separately in #3144 (method reification)
// and is intentionally NOT covered here.

function show(label: string, value: any) {
  console.log(label + ":", String(value));
}

// 1. Built-in constructor .length (backed by a shared runtime thunk → was 0).
show("Array.length", Array.length); // 1
show("Object.length", Object.length); // 1
show("String.length", String.length); // 1
show("Date.length", Date.length); // 7
show("RegExp.length", RegExp.length); // 2
show("Map.length", Map.length); // 0
show("Uint8Array.length", Uint8Array.length); // 3
show("Promise.length", Promise.length); // 1

// 1b. The pervasive cast idiom must fold identically.
show("(Array as any).length", (Array as any).length); // 1

// 1c. .name still folds (#2144) and is unchanged.
show("Array.name", Array.name); // Array

// 2. .name / .length on a built-in prototype method.
show("map.name", Array.prototype.map.name); // map
show("map.length", Array.prototype.map.length); // 1
show("forEach.name", Array.prototype.forEach.name); // forEach
show("slice.name", String.prototype.slice.name); // slice

// 3. Full descriptor facets — the proto method itself is writable:true (#2554),
//    its .name / .length are writable:false, configurable:true.
show(
  "proto-method desc",
  JSON.stringify(Object.getOwnPropertyDescriptor(Array.prototype, "map")),
);
show(
  "name desc",
  JSON.stringify(
    Object.getOwnPropertyDescriptor(Array.prototype.map, "name"),
  ),
);
show(
  "length desc",
  JSON.stringify(
    Object.getOwnPropertyDescriptor(Array.prototype.map, "length"),
  ),
);

// 4. writable:false enforcement — a sloppy-mode write to a non-writable
//    builtin descriptor must be a silent no-op.
(Array.prototype.map as any).name = "clobbered";
show("name after write", Array.prototype.map.name); // map
(Array.prototype.map as any).length = 999;
show("length after write", Array.prototype.map.length); // 1

// 4b. Plain (writable) function properties are unaffected by the descriptor gate.
function userFn() {}
(userFn as any).tag = 1;
(userFn as any).tag = 2;
show("userFn.tag", (userFn as any).tag); // 2

// 4c. A user defineProperty({writable:false}) on a function is honored too.
Object.defineProperty(userFn, "frozen", {
  value: 7,
  writable: false,
  configurable: true,
});
(userFn as any).frozen = 100;
show("userFn.frozen", (userFn as any).frozen); // 7
