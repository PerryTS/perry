import { Buffer } from "node:buffer";

// #2013: Buffer.byteLength rejects a non string/Buffer/ArrayBuffer/TypedArray
// first argument with ERR_INVALID_ARG_TYPE.
for (const value of [5, {}, null, true] as any[]) {
  try {
    Buffer.byteLength(value);
    console.log("byteLength", String(value), "=> NO THROW");
  } catch (err: any) {
    console.log("byteLength", String(value), "=>", err.name, err.code);
  }
}

// Valid argument shapes still return the right byte counts.
console.log("string:", Buffer.byteLength("héllo"));
console.log("buffer:", Buffer.byteLength(Buffer.from("abc")));
console.log("uint8array:", Buffer.byteLength(new Uint8Array(3)));
console.log("arraybuffer:", Buffer.byteLength(new ArrayBuffer(4)));
console.log("dataview:", Buffer.byteLength(new DataView(new ArrayBuffer(5))));
console.log("hex:", Buffer.byteLength("deadbeef", "hex"));
