// #6412 — a dynamic (any-typed) string key on a Buffer/Uint8Array stores and
// reads an OWN property, not a byte. Node's Buffer is an ordinary Uint8Array,
// so `buf[k]` with a non-numeric `k` is a property, and the numeric fast path
// (loop counters, literal indices) must stay a byte access.
const buf = Buffer.alloc(4);

// `k` is a string at runtime but `any` statically — the common shape.
const keys: any[] = ["dyn"];
const k: any = keys[0];
(buf as any)[k] = "D";
console.log("dyn-read:", (buf as any)[k]);
console.log("dyn-typeof:", typeof (buf as any)[k]);

// key out of a call result / Object.keys — also `any`.
function keyName(): any {
  return "prop";
}
(buf as any)[keyName()] = 42;
console.log("call-key-read:", (buf as any)["prop"]);

// numeric fast path preserved.
(buf as any)[0] = 0x41;
console.log("byte0:", buf[0]);
for (let i = 0; i < 4; i++) {
  buf[i] = (i * 3) & 0xff;
}
console.log("loop-bytes:", buf[0], buf[1], buf[2], buf[3]);

// a method value read through a dynamic key still binds (no own-prop shadow).
console.log("method-typeof:", typeof (buf as any)["readUInt8"]);

// a dynamic key that IS a canonical index still reads the byte — whether it
// arrives as a number or a canonical numeric-index STRING (`buf["2"]`).
const idx: any = 2;
console.log("dyn-index:", (buf as any)[idx]);
const sidx: any = "2";
console.log("dyn-str-index:", (buf as any)[sidx]);
const soob: any = "99";
console.log("dyn-str-oob:", (buf as any)[soob]);
