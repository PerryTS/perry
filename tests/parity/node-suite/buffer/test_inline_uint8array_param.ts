// Regression test for issue #169: SIGTRAP when a program mixes
// Buffer-typed and Uint8Array-typed function parameters.
//
// Root cause: `substitute_locals` in crates/perry-transform/src/inline.rs
// had no arms for Uint8ArrayGet/Set/Length/New, so inlining a function
// taking a Uint8Array parameter left a stale LocalGet(param_id) in the
// inlined body. The codegen's soft fallback boxed it as TAG_UNDEFINED,
// the slow-path bounds check then fired @llvm.assume(i1 false), and the
// program trapped with exit 133.
//
// Compare byte-for-byte against `node --experimental-strip-types`.

const big = Buffer.alloc(1024);
for (let i = 0; i < 256; i++) big.writeInt32BE(i * 37, i * 4);

function sumRow(row: Buffer, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += row.readInt32BE(i * 4);
  return s;
}
console.log("param sum:", sumRow(big, 256));

function firstBytes(arr: Uint8Array): number {
  return arr[0] + arr[1] + arr[2] + arr[3];
}
console.log("u8 param:", firstBytes(big));

// Reverse order — same param shapes, opposite call order.
function sumU8(arr: Uint8Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) s += arr[i];
  return s;
}
console.log("u8 sum:", sumU8(big, 16));

// Uint8ArrayLength via inlinable param.
function len(arr: Uint8Array): number {
  return arr.length;
}
console.log("u8 len:", len(big));

// Uint8ArraySet via inlinable param — write-then-read round trip.
function setFirst(arr: Uint8Array, v: number): void {
  arr[0] = v;
}
const small = new Uint8Array(4);
setFirst(small, 42);
console.log("u8 set:", small[0]);
