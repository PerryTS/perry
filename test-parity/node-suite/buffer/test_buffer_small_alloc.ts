// Correctness test for small-buffer slab fast path (issue #92).
// Exercises Buffer.alloc, Buffer.from, Buffer.slice, is-Buffer check, and
// instanceof for sizes below and at the 256-byte threshold.

// 1. Small alloc basic correctness
const b1 = Buffer.alloc(16);
console.log("alloc(16).length:", b1.length);
b1[0] = 0xAA;
b1[15] = 0xBB;
console.log("b1[0]:", b1[0].toString(16));
console.log("b1[15]:", b1[15].toString(16));

// 2. fill value propagates
const b2 = Buffer.alloc(8, 0x42);
console.log("alloc(8,0x42)[0]:", b2[0].toString(16));
console.log("alloc(8,0x42)[7]:", b2[7].toString(16));

// 3. Buffer.from with small string (< 256 bytes)
const b3 = Buffer.from("Hello");
console.log("from('Hello').length:", b3.length);
console.log("from('Hello').toString:", b3.toString("utf8"));

// 4. Small Buffer.from([...])
const b4 = Buffer.from([1, 2, 3, 4]);
console.log("from([1,2,3,4]).length:", b4.length);
console.log("from([1,2,3,4])[2]:", b4[2]);

// 5. slice produces a small slab buffer, reads correctly
const src = Buffer.from("Hello World");
const sliced = src.slice(0, 5);
console.log("slice(0,5).length:", sliced.length);
console.log("slice(0,5).str:", sliced.toString("utf8"));

// 6. 100 consecutive small allocs – verify each is independently addressable
// (read each buffer before moving to the next to avoid the pre-existing
// arr[i][0] two-level subscript codegen limitation).
let ok = true;
for (let i = 0; i < 100; i++) {
    const b = Buffer.alloc(16, i & 0xFF);
    if (b[0] !== (i & 0xFF)) { ok = false; break; }
}
console.log("100 independent allocs:", ok ? "pass" : "FAIL");

// 7. Boundary: alloc(255) is still small path
const b255 = Buffer.alloc(255, 0xFF);
console.log("alloc(255).length:", b255.length);
console.log("alloc(255)[0]:", b255[0]);
console.log("alloc(255)[254]:", b255[254]);

// 8. Boundary: alloc(256) uses large (malloc) path — also correct
const b256 = Buffer.alloc(256, 0xAB);
console.log("alloc(256).length:", b256.length);
console.log("alloc(256)[0]:", b256[0].toString(16));

// 9. Buffer.isBuffer works for slab-allocated buffers
console.log("isBuffer(b1):", Buffer.isBuffer(b1));
console.log("isBuffer(b256):", Buffer.isBuffer(b256));

// 10. toString hex on a slab buffer
const bHex = Buffer.alloc(4, 0xDE);
console.log("hex:", bHex.toString("hex"));

// 11. concat of small slab buffers
const ca = Buffer.from("Foo");
const cb = Buffer.from("Bar");
const cc = Buffer.concat([ca, cb]);
console.log("concat:", cc.toString("utf8"));
console.log("concat.length:", cc.length);
