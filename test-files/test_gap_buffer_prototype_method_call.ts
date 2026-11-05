// Buffer.prototype methods extracted as values and invoked with an explicit
// receiver, which may be a Buffer or a plain Uint8Array.
const proto: any = Buffer.prototype;

console.log("typeof utf8Slice:", typeof proto.utf8Slice);
console.log("typeof utf8Write:", typeof proto.utf8Write);
console.log("typeof toString:", typeof proto.toString);

const ascii = [104, 101, 108, 108, 111, 32, 119, 111, 114, 108, 100, 33, 33, 33, 33];
const multibyte = [0x68, 0xc3, 0xa9, 0xe2, 0x82, 0xac, 0xf0, 0x9f, 0x98, 0x80, 0x21];
const invalid = [0x61, 0xff, 0x62, 0xc3, 0x28, 0xe2, 0x82];

const buf = Buffer.from(ascii);
const bytes = new Uint8Array(ascii);

console.log("utf8Slice buffer:", proto.utf8Slice.call(buf, 1, 14));
console.log("utf8Slice uint8array:", proto.utf8Slice.call(bytes, 1, 14));
console.log("utf8Slice defaults:", proto.utf8Slice.call(bytes));
console.log("utf8Slice start only:", proto.utf8Slice.call(bytes, 6));
console.log("utf8Slice multibyte buffer:", proto.utf8Slice.call(Buffer.from(multibyte), 0, multibyte.length));
console.log("utf8Slice multibyte uint8array:", proto.utf8Slice.call(new Uint8Array(multibyte), 0, multibyte.length));

const replaced: string = proto.utf8Slice.call(new Uint8Array(invalid), 0, invalid.length);
console.log("utf8Slice invalid:", replaced, replaced.length, replaced.charCodeAt(1).toString(16));

const high = [0x41, 0xe9, 0xff, 0x7a];
console.log("latin1Slice buffer:", proto.latin1Slice.call(Buffer.from(high), 0, 4).split("").map((c: string) => c.charCodeAt(0)).join(","));
console.log("latin1Slice uint8array:", proto.latin1Slice.call(new Uint8Array(high), 0, 4).split("").map((c: string) => c.charCodeAt(0)).join(","));
console.log("asciiSlice uint8array:", proto.asciiSlice.call(bytes, 0, 5));
console.log("hexSlice buffer:", proto.hexSlice.call(buf, 0, 5));
console.log("hexSlice uint8array:", proto.hexSlice.call(bytes, 0, 5));
console.log("base64Slice buffer:", proto.base64Slice.call(buf, 0, 12));
console.log("base64Slice uint8array:", proto.base64Slice.call(bytes, 0, 12));
console.log("base64urlSlice uint8array:", proto.base64urlSlice.call(new Uint8Array([251, 255, 254]), 0, 3));
console.log("ucs2Slice uint8array:", proto.ucs2Slice.call(new Uint8Array([104, 0, 105, 0]), 0, 4));

console.log("toString hex buffer:", proto.toString.call(buf, "hex"));
console.log("toString hex uint8array:", proto.toString.call(bytes, "hex"));
console.log("toString utf8 range:", proto.toString.call(buf, "utf8", 0, 5));

const target = new Uint8Array(8);
const written = proto.utf8Write.call(target, "héllo", 1, 6);
console.log("utf8Write uint8array:", written, Array.from(target).join(","));
const targetBuffer = Buffer.alloc(6);
console.log("utf8Write buffer:", proto.utf8Write.call(targetBuffer, "abc", 2), targetBuffer.toString("hex"));
const hexTarget = new Uint8Array(4);
console.log("hexWrite uint8array:", proto.hexWrite.call(hexTarget, "deadbeef", 0, 4), Array.from(hexTarget).join(","));
console.log("write buffer:", proto.write.call(Buffer.alloc(5), "hi", 1));

const sliced = proto.slice.call(buf, 6, 11);
console.log("slice buffer:", Buffer.isBuffer(sliced), sliced.length, sliced.toString());
const subarray = proto.subarray.call(buf, 0, 5);
console.log("subarray buffer:", Buffer.isBuffer(subarray), subarray.toString());

console.log("apply utf8Slice:", proto.utf8Slice.apply(bytes, [0, 5]));
console.log("apply toString:", proto.toString.apply(buf, ["base64"]));
console.log("apply hexSlice no args:", proto.hexSlice.apply(new Uint8Array([1, 2, 255])));

const extracted = proto.utf8Slice;
console.log("extracted via bind:", extracted.bind(bytes)(0, 5));
console.log("extracted via Reflect.apply:", Reflect.apply(extracted, bytes, [6, 11]));

const readBuf = Buffer.from([0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0]);
console.log("readUInt8:", proto.readUInt8.call(readBuf, 1));
console.log("readUInt16BE:", proto.readUInt16BE.call(readBuf, 0));
console.log("readUInt32LE:", proto.readUInt32LE.call(readBuf, 2));
console.log("readInt16LE:", proto.readInt16LE.call(readBuf, 6));
const writeBuf = Buffer.alloc(6);
console.log("writeUInt16BE:", proto.writeUInt16BE.call(writeBuf, 0xabcd, 1), writeBuf.toString("hex"));
console.log("writeUInt32LE:", proto.writeUInt32LE.call(writeBuf, 0x01020304, 2), writeBuf.toString("hex"));
console.log("readUInt8 uint8array:", proto.readUInt8.call(new Uint8Array([7, 9]), 1));

try {
  proto.utf8Slice.call({}, 0, 1);
  console.log("non-buffer receiver: no throw");
} catch (error) {
  console.log("non-buffer receiver:", (error as Error).constructor.name);
}
