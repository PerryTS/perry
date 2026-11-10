// #4103 — `new TA(buffer, byteOffset, length)` must validate `byteOffset` /
// `length` against the backing `ArrayBuffer` and throw a `RangeError` for an
// out-of-range / misaligned view (it previously succeeded silently). We print
// the error *type* (or the resulting length for the valid cases) so the output
// is byte-identical to Node.

function r(fn: () => unknown): string {
    try {
        return "ok:" + String(fn());
    } catch (e: any) {
        return "throw:" + e.constructor.name;
    }
}

// Out-of-range / misaligned → RangeError.
console.log("u8 7,4:", r(() => new Uint8Array(new ArrayBuffer(8), 7, 4))); // offset+len > buffer
console.log("u32 3:", r(() => new Uint32Array(new ArrayBuffer(8), 3))); // offset % 4 != 0
console.log("u8 16:", r(() => new Uint8Array(new ArrayBuffer(8), 16))); // offset > byteLength
console.log("u32 6,1:", r(() => new Uint32Array(new ArrayBuffer(8), 6, 1))); // offset misaligned
console.log("u16 7:", r(() => new Uint16Array(new ArrayBuffer(8), 7))); // odd offset for 2-byte

// Valid views → length of the view.
console.log("u8 4,4:", r(() => new Uint8Array(new ArrayBuffer(8), 4, 4).length)); // 4
console.log("u32 4:", r(() => new Uint32Array(new ArrayBuffer(8), 4).length)); // 1
console.log("u32 0:", r(() => new Uint32Array(new ArrayBuffer(8), 0).length)); // 2
console.log("u16 2,2:", r(() => new Uint16Array(new ArrayBuffer(8), 2, 2).length)); // 2

// Data is read from the byte offset.
const ab = new ArrayBuffer(8);
const bytes = new Uint8Array(ab);
bytes[4] = 0xaa;
bytes[5] = 0xbb;
const view = new Uint8Array(ab, 4, 2);
console.log("data:", view[0], view[1]); // 170 187
