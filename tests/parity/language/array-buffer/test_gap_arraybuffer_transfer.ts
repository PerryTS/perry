// ArrayBuffer.prototype.transfer / transferToFixedLength / detached (ES2024).
// Compared byte-for-byte against `node --experimental-strip-types`.

// Basic transfer: contents move, source detaches.
const ab = new ArrayBuffer(8);
const u8 = new Uint8Array(ab);
u8[0] = 42;
u8[7] = 7;
console.log(ab.byteLength, ab.detached);

const moved = ab.transfer();
console.log(moved.byteLength, moved.detached);
const mv = new Uint8Array(moved);
console.log(mv[0], mv[7]);
console.log(ab.byteLength, ab.detached);
console.log(u8.length);

// Growing transfer zero-fills the tail.
const grown = moved.transfer(16);
const gv = new Uint8Array(grown);
console.log(grown.byteLength, gv[0], gv[7], gv[8], gv[15]);

// Shrinking transfer truncates.
const shrunk = grown.transfer(4);
const sv = new Uint8Array(shrunk);
console.log(shrunk.byteLength, sv.length, sv[0]);

// transferToFixedLength behaves like transfer (no resizable buffers here).
const fixed = shrunk.transferToFixedLength(6);
console.log(fixed.byteLength, fixed.detached, shrunk.detached);

// slice() copies survive a later detach of their source.
const src = new ArrayBuffer(4);
new Uint8Array(src)[0] = 9;
const copy = src.slice(0);
src.transfer();
console.log(copy.byteLength, new Uint8Array(copy)[0]);

// Error cases.
try {
  shrunk.transfer();
} catch (e) {
  console.log("re-transfer:", (e as Error).name);
}
try {
  fixed.transfer(-1);
} catch (e) {
  console.log("negative-length:", (e as Error).name);
}
try {
  shrunk.slice(0);
} catch (e) {
  console.log("slice-detached:", (e as Error).name);
}
try {
  new Uint8Array(shrunk);
} catch (e) {
  console.log("view-detached:", (e as Error).name);
}
try {
  new DataView(shrunk);
} catch (e) {
  console.log("dataview-detached:", (e as Error).name);
}

// structuredClone transfer detaches the source the same way.
const big = new ArrayBuffer(32);
new Uint8Array(big)[0] = 5;
const cloned = structuredClone(big, { transfer: [big] });
console.log(cloned.byteLength, new Uint8Array(cloned)[0], big.detached, big.byteLength);
try {
  structuredClone(big, { transfer: [big] });
} catch (e) {
  console.log("re-clone:", (e as Error).name);
}

// Coercion ordering: ToIndex(newLength/byteOffset) runs BEFORE the detached
// check and can itself detach the buffer via valueOf (spec ordering).
const vb = new ArrayBuffer(8);
try {
  vb.transfer({
    valueOf() {
      vb.transfer();
      return 4;
    },
  } as any);
} catch (e) {
  console.log("transfer-valueof-detach:", (e as Error).name);
}
const dvb = new ArrayBuffer(8);
try {
  new DataView(dvb, {
    valueOf() {
      dvb.transfer();
      return 0;
    },
  } as any);
} catch (e) {
  console.log("dataview-valueof-detach:", (e as Error).name);
}
const tvb = new ArrayBuffer(8);
try {
  new Uint8Array(tvb, {
    valueOf() {
      tvb.transfer();
      return 0;
    },
  } as any);
} catch (e) {
  console.log("typedarray-valueof-detach:", (e as Error).name);
}

// Cloning an already-detached buffer (no transfer list) throws.
const det = new ArrayBuffer(4);
det.transfer();
try {
  structuredClone(det);
} catch (e) {
  console.log("clone-detached:", (e as Error).name);
}

// A clone that FAILS must leave transfer-list buffers attached.
const keep = new ArrayBuffer(4);
try {
  structuredClone({ b: keep, f: () => 1 }, { transfer: [keep] });
} catch (e) {
  console.log("failed-clone-keeps:", (e as Error).name, keep.detached, keep.byteLength);
}
