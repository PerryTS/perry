// parity-node-argv: --expose-gc
"use strict";
export {};
declare function gc(): void;
function collect(): void { if (typeof gc === "function") gc(); }

// Warm one free-variable read site, then mutate the slot and its descriptor.
const original: any = Array;
function readGlobal(): any { return Array; }
for (let i = 0; i < 20; i++) {
  if (readGlobal() !== original) throw new Error("initial global");
}
const replacement: any = function Replacement() {};
(globalThis as any).Array = replacement;
collect();
console.log("reassigned", readGlobal() === replacement);
(globalThis as any).Array = original;
console.log("restored", readGlobal() === original);
let calls = 0;
Object.defineProperty(globalThis, "Array", {
  configurable: true,
  get() { calls++; return replacement; },
});
collect();
console.log("accessor", readGlobal() === replacement, readGlobal() === replacement, calls);
Object.defineProperty(globalThis, "Array", {
  configurable: true, writable: true, enumerable: false, value: original,
});
console.log("data again", readGlobal() === original);
// Lexical names and captured lexical names never enter the global read site.
{
  let Array: any = 37;
  function readLocal(): any { return Array; }
  console.log("local", readLocal());
  Array = 41;
  console.log("captured local", readLocal());
}
function parameter(Array: any): any { return Array; }
console.log("parameter", parameter(43), "global", readGlobal() === original);

const savedObject: any = Object;
const savedStatic: any = Object.hasOwn;
function readStatic(): any { return Object.hasOwn; }
for (let i = 0; i < 20; i++) {
  if (readStatic() !== savedStatic) throw new Error("initial static");
}
const fakeObject: any = function FakeObject() {};
fakeObject.hasOwn = replacement;
(globalThis as any).Object = fakeObject;
collect();
console.log("static receiver reassigned", readStatic() === replacement);
(globalThis as any).Object = savedObject;
console.log("static receiver restored", readStatic() === savedStatic);
