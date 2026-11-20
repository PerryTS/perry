import { Buffer } from "node:buffer";

function named(view: Uint8Array): string {
  return `${view.length}:${view.byteLength}:${view.byteOffset}`;
}
function keyed(view: any, key: string): any { return view[key]; }

const owner = Buffer.alloc(4);
owner.set([10, 20, 30, 40]);
console.log("buffer", named(owner), named(owner.subarray(1, 3)));
const backing = new ArrayBuffer(32);
const words = new Uint32Array(backing, 8, 3);
console.log("words", words.length, words.byteLength, words.byteOffset);
for (const key of ["length", "byteLength", "byteOffset"]) {
  console.log("dynamic", key, keyed(words, key), keyed(owner, key));
}
Object.defineProperty(owner, "length", { value: 99, configurable: true });
console.log("own", named(owner), keyed(owner, "length"));
delete (owner as any).length;
let calls = 0;
Object.defineProperty(words, "byteLength", {
  get() {
    calls++;
    if (typeof gc === "function") gc();
    return this[0] + 17;
  }, configurable: true,
});
words[0] = 9;
console.log("accessor", words.byteLength, keyed(words, "byteLength"), calls);
delete (words as any).byteLength;
const other = new Uint8Array([4, 5]);
Object.setPrototypeOf(other, {
  get length() { return this[0] + 20; }, byteLength: "custom", byteOffset: 7,
});
console.log("custom", named(other), keyed(other, "length"));
const nullProto = new Uint8Array(3);
Object.setPrototypeOf(nullProto, null);
console.log("null proto", named(nullProto));
const protoBuffer = Buffer.alloc(1, 6);
const offsetBuffer = Buffer.alloc(1, 8);
const proto = Object.getPrototypeOf(Uint8Array.prototype);
const saved = Object.getOwnPropertyDescriptor(proto, "length")!;
Object.defineProperty(proto, "length", {
  get() { if (typeof gc === "function") gc(); return this[0] + 40; }, configurable: true,
});
const protoTypedResult = named(new Uint8Array([2, 3]));
const protoBufferResult = named(protoBuffer);
Object.defineProperty(proto, "length", saved);
console.log("prototype", protoTypedResult, protoBufferResult);
Object.defineProperty(Buffer.prototype, "byteOffset", {
  get() { return this[0] + 50; }, configurable: true,
});
const offsetResult = named(offsetBuffer);
delete (Buffer.prototype as any).byteOffset;
console.log("buffer prototype", offsetResult);
console.log("ordinary", named({ length: "text", byteLength: 2, byteOffset: 3 } as any));
const resizable = new ArrayBuffer(16, { maxByteLength: 32 });
const tracking = new Uint8Array(resizable, 4);
const fixed = new Uint8Array(resizable, 4, 8);
console.log("resize before", named(tracking), named(fixed));
resizable.resize(6);
console.log("resize small", named(tracking), named(fixed));
resizable.resize(20);
console.log("resize large", named(tracking), named(fixed));
resizable.transfer();
console.log("detach", named(tracking), named(fixed), tracking[0], tracking[-1]);
