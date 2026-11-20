// Redefining a typed-array prototype's metadata accessors or its kind
// prototype's `constructor` changes what every typed array reads, at a site
// that already read the defaults.
function read(o: any): string {
  const c = o.constructor;
  return [o.length, o.byteLength, o.byteOffset, o.BYTES_PER_ELEMENT, typeof c === "function" ? c.name : String(c)].join(" ");
}
const f = new Float32Array(4);
const i = new Int8Array(3);
const v = new Float32Array(new ArrayBuffer(32), 8, 2);
console.log(read(f), "|", read(i), "|", read(v));

Object.defineProperty(Float32Array.prototype, "length", { get: () => 7, configurable: true });
console.log("f32 length", read(f), "|", read(i), "|", read(v));

const TAProto = Object.getPrototypeOf(Int8Array.prototype);
Object.defineProperty(TAProto, "byteOffset", { get: () => 42, configurable: true });
console.log("byteOffset", read(f), "|", read(i), "|", read(v));

(Int8Array.prototype as any).constructor = "replaced";
console.log("ctor written", read(f), "|", read(i));
delete (Float32Array.prototype as any).constructor;
console.log("ctor deleted", read(f), "|", read(i));

try {
  (Float32Array.prototype as any).BYTES_PER_ELEMENT = 9;
  console.log("no throw");
} catch (e) {
  console.log("BPE write", (e as Error).constructor.name);
}
console.log("bpe", read(f), "|", read(i));
