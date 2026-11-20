// #7700: a non-numeric key on a Uint8Array/Buffer-typed local must read a
// PROPERTY, not a byte. `lower/expr_member/member_tail.rs` folds every
// non-STRING key on such a local onto `Expr::Uint8ArrayGet`, and the codegen
// collectors then classified the destination local as integer-valued no matter
// what the key was — so `const it = u8[Symbol.iterator]` took an i32 slot and
// `ToInt32(ToNumber(fn))` reported `typeof it === "number"`.
//
// Every case below stores the read in a LOCAL: that is what selects the
// representation, and the direct-consumption form (`typeof u8[Symbol.iterator]`)
// was already correct.

const u8 = new Uint8Array([1, 2, 3, 4]);

// A symbol key reads the iterator method.
const it = u8[Symbol.iterator];
console.log("iterator:", typeof it);

// An `any`-typed key holding a method name reads the method.
const methodKey: any = "subarray";
const method = u8[methodKey];
console.log("subarray:", typeof method);

// …a length accessor reads the length.
const lenKey: any = "byteLength";
const byteLength = u8[lenKey];
console.log("byteLength:", byteLength);

// …and an own expando reads the expando.
const anyU8: any = u8;
anyU8.tag = { kind: "buffer" };
const tagKey: any = "tag";
const tag = u8[tagKey];
console.log("tag:", JSON.stringify(tag));

// A BigInt expando must not be classified non-BigInt either.
anyU8.big = 7n;
const bigKey: any = "big";
const big = u8[bigKey];
console.log("big:", typeof big, String(big));

// Buffer is a Uint8Array subclass and folds the same way.
const buf = Buffer.from([9, 8, 7]);
const bufIt = buf[Symbol.iterator];
console.log("buffer iterator:", typeof bufIt);
const bufWrite: any = "writeUInt8";
const bufMethod = buf[bufWrite];
console.log("buffer method:", typeof bufMethod);

// The numeric-key byte read is unchanged — including the loop shape whose i32
// representation this fix must not cost.
const i = 2;
const b = u8[i];
console.log("byte:", b);

let sum = 0;
for (let k = 0; k < u8.length; k++) {
  sum += u8[k];
}
console.log("sum:", sum);

let masked = 0;
for (let k = 0; k < 8; k++) {
  masked = (masked + u8[k & 3]) | 0;
}
console.log("masked:", masked);

// Iteration still works through the real iterator.
console.log("spread:", JSON.stringify([...u8]));
