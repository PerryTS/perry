// Two-phase realm: the first touch of the realm is node's Buffer (a phase-B
// host family, a Uint8Array subclass), then its links to the typed arrays.
const buf = Buffer.alloc(4);
buf.writeUInt16BE(0x1234, 0);
console.log("alloc", buf[0], buf.readUInt16BE(0), buf.length, buf.slice(0, 2).length);
const b = Buffer.from("abc");
console.log("from", b.toString("hex"), b.toString("base64"), Buffer.isBuffer(b), Buffer.byteLength("hé"));
console.log("links", b instanceof Uint8Array, Object.getPrototypeOf(Buffer.prototype) === Uint8Array.prototype, Object.getPrototypeOf(Buffer) === Uint8Array);
console.log("ctor", (b as any).constructor === Buffer, Buffer === (globalThis as any).Buffer, typeof Buffer.concat);
console.log("concat", Buffer.concat([b, Buffer.from("de")]).toString(), b.subarray(1).toString());
console.log("typed", new Uint8Array(b).join(","), Object.prototype.toString.call(b), ArrayBuffer.isView(b), b.buffer instanceof ArrayBuffer);
class After { static ok = true; }
console.log("core after", [After][0].ok, typeof Map, typeof WeakMap, typeof Intl);
