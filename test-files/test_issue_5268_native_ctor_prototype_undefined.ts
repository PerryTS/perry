// #5268: native-module constructor *class* exports must expose a real
// `.prototype` object so the userland subclass idioms
//
//     ReadStream.prototype = Object.create(fs$ReadStream.prototype)  // graceful-fs
//     Object.setPrototypeOf(prototype, EventEmitter.prototype)        // pino
//
// don't throw "Object prototype may only be an Object or null: undefined".
//
// Pre-fix, `fs.ReadStream`/`fs.WriteStream`/`events.EventEmitter` were
// truthy callable closures (bound-native exports) whose `.prototype`
// read resolved to `undefined`: `ordinary_function_prototype_value_for_read`
// short-circuited to `None` for every bound-native export except a
// hardcoded http/https whitelist. graceful-fs guards on truthiness of
// `fs$ReadStream` (truthy) then calls `Object.create(fs$ReadStream.prototype)`
// → `Object.create(undefined)` → throw. The fix recognizes constructor-cased
// bound-native exports (leading uppercase, not flagged non-constructable) and
// lets the synthetic-class path materialize a stable `.prototype` object.

import * as fs from "node:fs";
import { EventEmitter } from "node:events";

// 1. Native constructor classes expose an object `.prototype` (not undefined).
const rsProto = (fs as any).ReadStream.prototype;
const wsProto = (fs as any).WriteStream.prototype;
const eeProto = (EventEmitter as any).prototype;
console.log("fs.ReadStream.prototype is object:", typeof rsProto === "object" && rsProto !== null);
console.log("fs.WriteStream.prototype is object:", typeof wsProto === "object" && wsProto !== null);
console.log("EventEmitter.prototype is object:", typeof eeProto === "object" && eeProto !== null);

// 2. graceful-fs's clone() ReadStream pattern: `Object.create(Ctor.prototype)`
//    must not throw and must yield an object whose proto is Ctor.prototype.
const sub = Object.create((fs as any).ReadStream.prototype);
console.log("Object.create(ReadStream.prototype) ok:", typeof sub === "object" && sub !== null);
console.log(
  "subclass proto chains to ReadStream.prototype:",
  Object.getPrototypeOf(sub) === (fs as any).ReadStream.prototype,
);

// 3. pino's proto.js pattern: `Object.setPrototypeOf(prototype, EventEmitter.prototype)`
//    must not throw on the (now real) EventEmitter.prototype operand.
const prototype: any = { child() { return null; } };
Object.setPrototypeOf(prototype, (EventEmitter as any).prototype);
console.log("setPrototypeOf(obj, EventEmitter.prototype) ok:", true);

// 4. Non-constructor native exports keep `prototype === undefined`, matching
//    Node's built-in non-constructor functions (no spurious synthesis).
console.log("fs.readFile.prototype is undefined:", (fs as any).readFile.prototype === undefined);
