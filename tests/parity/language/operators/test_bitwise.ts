// Test bitwise operators

// Bitwise AND (&)
let and1 = 12 & 10;   // 1100 & 1010 = 1000 = 8
console.log(and1);     // Should be 8

let and2 = 255 & 15;  // 11111111 & 00001111 = 00001111 = 15
console.log(and2);     // Should be 15

// Bitwise OR (|)
let or1 = 12 | 10;    // 1100 | 1010 = 1110 = 14
console.log(or1);      // Should be 14

let or2 = 4 | 1;      // 0100 | 0001 = 0101 = 5
console.log(or2);      // Should be 5

// Bitwise XOR (^)
let xor1 = 12 ^ 10;   // 1100 ^ 1010 = 0110 = 6
console.log(xor1);     // Should be 6

let xor2 = 5 ^ 3;     // 0101 ^ 0011 = 0110 = 6
console.log(xor2);     // Should be 6

// Bitwise NOT (~)
let not1 = ~0;        // ~0 = -1 (all bits flipped)
console.log(not1);     // Should be -1

let not2 = ~1;        // ~1 = -2
console.log(not2);     // Should be -2

let not3 = ~(-1);     // ~(-1) = 0
console.log(not3);     // Should be 0

// Left shift (<<)
let shl1 = 1 << 4;    // 0001 << 4 = 10000 = 16
console.log(shl1);     // Should be 16

let shl2 = 5 << 2;    // 0101 << 2 = 10100 = 20
console.log(shl2);     // Should be 20

// Signed right shift (>>)
let shr1 = 16 >> 2;   // 10000 >> 2 = 00100 = 4
console.log(shr1);     // Should be 4

let shr2 = -16 >> 2;  // Signed shift preserves sign
console.log(shr2);     // Should be -4

// Unsigned right shift (>>>)
let ushr1 = 16 >>> 2; // 10000 >>> 2 = 00100 = 4
console.log(ushr1);    // Should be 4

// Unsigned right shift on negative number
// -1 in 32-bit is 0xFFFFFFFF
// >>> 0 converts to unsigned 32-bit
let ushr2 = -1 >>> 0;
console.log(ushr2);    // Should be 4294967295

// Compound assignment operators
let ca = 12;
ca &= 10;              // 12 & 10 = 8
console.log(ca);       // Should be 8

let co = 12;
co |= 10;              // 12 | 10 = 14
console.log(co);       // Should be 14

let cx = 12;
cx ^= 10;              // 12 ^ 10 = 6
console.log(cx);       // Should be 6

let cl = 1;
cl <<= 4;              // 1 << 4 = 16
console.log(cl);       // Should be 16

let cr = 16;
cr >>= 2;              // 16 >> 2 = 4
console.log(cr);       // Should be 4

let cu = 16;
cu >>>= 2;             // 16 >>> 2 = 4
console.log(cu);       // Should be 4

// Combined expressions
let combined = (5 | 3) & ~1;  // (0111) & (1110) = 0110 = 6
console.log(combined);         // Should be 6

// Flag operations (common use case)
const FLAG_A = 1;      // 0001
const FLAG_B = 2;      // 0010
const FLAG_C = 4;      // 0100

let flags = 0;
flags |= FLAG_A;       // Set FLAG_A
flags |= FLAG_C;       // Set FLAG_C
console.log(flags);    // Should be 5 (0101)

let hasA = (flags & FLAG_A) !== 0;
let hasB = (flags & FLAG_B) !== 0;
console.log(hasA);     // Should be 1 (true)
console.log(hasB);     // Should be 0 (false)

flags &= ~FLAG_A;      // Clear FLAG_A
console.log(flags);    // Should be 4 (0100)
