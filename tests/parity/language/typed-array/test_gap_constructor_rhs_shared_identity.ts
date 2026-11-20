// parity-node-argv: --expose-gc
"use strict";
export {};
declare function gc(): void;
const U: any = Uint8Array;
const A: any = ArrayBuffer;
const originalName: any = Object.getOwnPropertyDescriptor(U, "name");
const originalBufferName: any = Object.getOwnPropertyDescriptor(A, "name");
const value: any = new Uint8Array(2);
const buffer: any = new ArrayBuffer(8);
let reads = 0;
Object.defineProperty(U, "name", { configurable: true, get() { reads++; if (typeof gc === "function") gc(); return "Set"; } });
Object.defineProperty(A, "name", { configurable: true, get() { reads++; if (typeof gc === "function") gc(); return "Set"; } });
if (typeof gc === "function") gc();
console.log("identity", value instanceof Uint8Array, value instanceof U, buffer instanceof A, reads);
console.log("construct", new U(3).length, new A(4).byteLength, reads);
// The identity survives replacement of the public binding and moving GC.
(globalThis as any).Uint8Array = function Replacement() {};
if (typeof gc === "function") gc();
console.log("alias", value instanceof U, value instanceof Uint8Array, reads);
(globalThis as any).Uint8Array = U;
console.log("public getters", U.name, A.name, reads);
console.log("identity again", value instanceof U, buffer instanceof A, reads);
Object.defineProperty(U, "name", originalName);
Object.defineProperty(A, "name", originalBufferName);
// A matching public label cannot turn an ordinary function into an intrinsic.
function Pretender() {}
Object.defineProperty(Pretender, "name", { configurable: true, value: "Uint8Array" });
console.log("ordinary", value instanceof Pretender, [] instanceof Pretender);
