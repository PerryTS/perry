import { Buffer } from "node:buffer";
function fields(view: any): string { return `${view.length}:${view.byteLength}:${view.byteOffset}`; }
const b = Buffer.alloc(8);
console.log("buffer", fields(b), fields(b.subarray(2, 6)));
const rab = new ArrayBuffer(32, { maxByteLength: 64 });
const bytes = new Uint8Array(rab, 4);
const words = new Uint32Array(rab, 8);
const fixed = new Uint32Array(rab, 8, 3);
console.log("before", fields(bytes), fields(words), fields(fixed));
rab.resize(10);
console.log("shrink", fields(bytes), fields(words), fields(fixed));
rab.resize(48);
console.log("grow", fields(bytes), fields(words), fields(fixed));
rab.transfer();
console.log("detach", fields(bytes), fields(words), fields(fixed));
console.log(bytes[0], words[0], fixed[-1]);

function Alternate() {}
Alternate.prototype = { length: "alternate", byteLength: 23, byteOffset: 19 };
const constructed = Reflect.construct(Uint8Array, [4], Alternate);
console.log("newTarget", fields(constructed));

const originalUint8Array = Uint8Array;
const intrinsicBufferPrototype = Buffer.prototype;
(globalThis as any).Uint8Array = function ReplacementUint8Array() {};
const retained = Buffer.alloc(2, 21);
(Buffer as any).prototype = { length: "replacement", byteLength: 31, byteOffset: 29 };
const fresh = Buffer.alloc(2, 22);
Object.defineProperty(intrinsicBufferPrototype, "length", {
  get() { if (typeof gc === "function") gc(); return this[0] + 90; }, configurable: true,
});
const retainedResult = fields(retained);
const freshResult = fields(fresh);
delete (intrinsicBufferPrototype as any).length;
(Buffer as any).prototype = intrinsicBufferPrototype;
(globalThis as any).Uint8Array = originalUint8Array;
console.log("intrinsic buffer prototype", retainedResult, freshResult,
  Object.getPrototypeOf(retained) === intrinsicBufferPrototype,
  Object.getPrototypeOf(intrinsicBufferPrototype) === originalUint8Array.prototype);
