// #6405 — an own property SHADOWS the same-named Buffer.prototype method even
// when the call statically folds to the inline byte-read intrinsic. A Buffer is
// an ordinary Uint8Array, so `b.readUInt8 = fn` overrides `readUInt8`.
const b = Buffer.alloc(8);
b[0] = 0xab;
(b as any).readUInt8 = function () {
  return "shadowed";
};

// All three call shapes must resolve to the own property.
console.log("dot:", (b as any).readUInt8(0));
console.log("literal-key:", (b as any)["readUInt8"](0));
const k = "readUInt8";
console.log("dyn-key:", (b as any)[k](0));

// A method value read (not a call) also returns the override.
console.log("value-typeof:", typeof (b as any).readUInt8);
console.log("value-call:", ((b as any).readUInt8 as () => string)());

// An UNSHADOWED buffer still reads the real bytes (via runtime dispatch here,
// since this module shadows a read method somewhere).
const c = Buffer.alloc(4);
c[0] = 0x7f;
c[1] = 0x10;
console.log("unshadowed-u8:", c.readUInt8(0));
console.log("unshadowed-i16be:", c.readInt16BE(0));

// A DIFFERENT read method on the shadowing buffer is not overridden.
console.log("other-method:", (b as any).readInt8(0));
