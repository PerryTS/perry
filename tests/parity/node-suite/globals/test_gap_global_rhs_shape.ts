"use strict";
export {};
const original: any = Array;
const object: any = [];
function read(): any { return Array; }
function check(value: any): boolean { return value instanceof Array; }
for (let i = 0; i < 30; i++) {
  if (read() !== original || !check(object)) throw new Error("warmup");
}
function kind(): string { return typeof Array; }
console.log("kind", kind());
const replacement: any = function Replacement() {};
(globalThis as any).Array = 37;
console.log("kind changed", kind());
(globalThis as any).Array = replacement;
console.log("replace", read() === replacement, check(object));
(globalThis as any).Array = original;
console.log("restore", check(object));
{
  let Array: any = replacement;
  function shadow(value: any): boolean { return value instanceof Array; }
  console.log("shadow", shadow(object));
  Array = original;
  console.log("shadow changed", shadow(object));
}
function parameter(Array: any, value: any): boolean { return value instanceof Array; }
console.log("parameter", parameter(replacement, object), parameter(original, object));
delete (globalThis as any).Array;
console.log("optional deleted", kind(), (globalThis as any).Array === undefined);
try { read(); console.log("deleted read missed"); }
catch (e: any) { console.log("deleted read", e.name); }
try { check(object); console.log("deleted rhs missed"); }
catch (e: any) { console.log("deleted rhs", e.name); }
(globalThis as any).Array = undefined;
console.log("present undefined", read() === undefined);
try { check(object); console.log("undefined rhs missed"); }
catch (e: any) { console.log("undefined rhs", e.name); }
(globalThis as any).Array = original;
// A runtime-created free variable uses the same site as a builtin binding.
(globalThis as any).qs2Value = 11;
function custom(): any { return qs2Value; }
console.log("custom", custom());
(globalThis as any).qs2Value = 17;
console.log("custom changed", custom());
delete (globalThis as any).qs2Value;
try { custom(); console.log("custom deletion missed"); }
catch (e: any) { console.log("custom deleted", e.name); }

let getterCalls = 0;
Object.defineProperty(globalThis, "qs2Undefined", {
  configurable: true,
  get() { getterCalls++; delete (globalThis as any).qs2Undefined; return undefined; }
});
function getterRead(): any { return qs2Undefined; }
console.log("deleting getter", getterRead() === undefined, getterCalls);
try { getterRead(); console.log("getter deletion missed"); }
catch (e: any) { console.log("getter deleted", e.name, getterCalls); }
