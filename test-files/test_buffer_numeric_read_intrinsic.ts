// Regression test for issue #92: Buffer numeric reads must return correct
// values when lowered via the codegen intrinsic fast path (inline LLVM
// load + bswap + convert instead of js_native_call_method → runtime).
//
// Both the intrinsic path (const Buffer.alloc local) and the fallback
// runtime path must agree with Node. Compare byte-for-byte against
// `node --experimental-strip-types`.

const buf = Buffer.alloc(64);

// --- Int8 / UInt8 ---
buf.writeInt8(-128, 0);
buf.writeInt8(127, 1);
buf.writeUInt8(0, 2);
buf.writeUInt8(255, 3);
console.log("i8  min/max:", buf.readInt8(0), buf.readInt8(1));
console.log("u8  min/max:", buf.readUInt8(2), buf.readUInt8(3));

// --- Int16 BE/LE (sign-extension matters) ---
buf.writeInt16BE(-1, 4);        // 0xFFFF
buf.writeInt16BE(32767, 6);     // 0x7FFF
buf.writeInt16LE(-32768, 8);    // 0x0080 (LE of 0x8000)
buf.writeInt16LE(12345, 10);
console.log("i16 BE -1:", buf.readInt16BE(4));
console.log("i16 BE max:", buf.readInt16BE(6));
console.log("i16 LE min:", buf.readInt16LE(8));
console.log("i16 LE mid:", buf.readInt16LE(10));

// --- UInt16 BE/LE (must NOT sign-extend 0xFFFF) ---
buf.writeUInt16BE(0xFFFF, 12);
buf.writeUInt16LE(0xFFFF, 14);
console.log("u16 BE 0xFFFF:", buf.readUInt16BE(12));
console.log("u16 LE 0xFFFF:", buf.readUInt16LE(14));

// --- Int32 BE/LE ---
buf.writeInt32BE(-2147483648, 16);
buf.writeInt32BE(2147483647, 20);
buf.writeInt32LE(-1, 24);
buf.writeInt32LE(305419896, 28);  // 0x12345678
console.log("i32 BE min:", buf.readInt32BE(16));
console.log("i32 BE max:", buf.readInt32BE(20));
console.log("i32 LE -1:", buf.readInt32LE(24));
console.log("i32 LE 0x12345678:", buf.readInt32LE(28));

// --- UInt32 BE/LE (must NOT sign-extend 0xFFFFFFFF) ---
buf.writeUInt32BE(0xFFFFFFFF, 32);
buf.writeUInt32LE(0xDEADBEEF, 36);
console.log("u32 BE 0xFFFFFFFF:", buf.readUInt32BE(32));
console.log("u32 LE 0xDEADBEEF:", buf.readUInt32LE(36));

// --- Float BE/LE ---
buf.writeFloatBE(3.14, 40);
buf.writeFloatLE(-0.5, 44);
console.log("f32 BE pi~:", buf.readFloatBE(40).toFixed(4));
console.log("f32 LE -0.5:", buf.readFloatLE(44));

// --- Double BE/LE ---
buf.writeDoubleBE(Math.PI, 48);
buf.writeDoubleLE(-123.456789, 56);
console.log("f64 BE pi:", buf.readDoubleBE(48));
console.log("f64 LE neg:", buf.readDoubleLE(56));

// --- Hot loop (intrinsic path via `const buf = Buffer.alloc(N)`) ---
const big = Buffer.alloc(1024);
for (let i = 0; i < 256; i++) big.writeInt32BE(i * 37, i * 4);
let sum = 0;
for (let i = 0; i < 256; i++) sum += big.readInt32BE(i * 4);
console.log("hot loop sum:", sum);  // 37 * sum(0..255) = 37 * 32640 = 1207680

// --- Buffer-typed function parameter (the Postgres driver shape).
// Covered by the buffer_data_slots extension in codegen.rs: params typed
// `Buffer` that aren't reassigned get a pre-computed data_ptr at function
// entry so `row.readInt32BE(off)` hits the intrinsic instead of runtime
// dispatch.
function sumRow(row: Buffer, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += row.readInt32BE(i * 4);
  return s;
}
console.log("param sum:", sumRow(big, 256));
