// #10873: resizable ArrayBuffer (ES2024) — `new ArrayBuffer(len, { maxByteLength })`,
// `ArrayBuffer.prototype.resize`, `resizable` / `maxByteLength`, length-tracking
// and fixed-length views (typed arrays + DataView), `transfer` preserving
// resizability. Compared byte-for-byte against `node --experimental-strip-types`.

function name(f: () => unknown): string {
  try {
    f();
    return "no throw";
  } catch (e) {
    return (e as Error).name;
  }
}

// --- the issue's repro -------------------------------------------------------
const b = new ArrayBuffer(0, { maxByteLength: 1024 });
console.log(typeof (b as any).resize, b.byteLength, b.resizable, b.maxByteLength);
b.resize(16);
console.log(b.byteLength, b.resizable, b.maxByteLength);

// --- a fixed-length buffer is unchanged --------------------------------------
const fixed = new ArrayBuffer(8);
console.log(fixed.resizable, fixed.maxByteLength, fixed.byteLength);
console.log("resize fixed:", name(() => fixed.resize(4)));
const noMax = new ArrayBuffer(8, {} as any);
console.log(noMax.resizable, noMax.maxByteLength);
const undefMax = new ArrayBuffer(8, { maxByteLength: undefined });
console.log(undefMax.resizable, undefMax.maxByteLength);
const nonObject = new ArrayBuffer(8, 5 as any);
console.log(nonObject.resizable, nonObject.maxByteLength);

// --- constructor validation + evaluation order -------------------------------
console.log("len > max:", name(() => new ArrayBuffer(10, { maxByteLength: 5 })));
console.log("negative max:", name(() => new ArrayBuffer(0, { maxByteLength: -1 })));
console.log("len == max:", new ArrayBuffer(5, { maxByteLength: 5 }).byteLength);
const order: string[] = [];
const ordered = new ArrayBuffer(
  {
    valueOf() {
      order.push("length");
      return 2;
    },
  } as any,
  {
    get maxByteLength() {
      order.push("get max");
      return {
        valueOf() {
          order.push("max valueOf");
          return 8;
        },
      } as any;
    },
  },
);
console.log(order.join(" > "), ordered.byteLength, ordered.maxByteLength);

// --- resize: grow zero-fills, shrink truncates, regrow re-zeroes -------------
const rab = new ArrayBuffer(4, { maxByteLength: 16 });
const tracking = new Uint8Array(rab);
tracking.set([1, 2, 3, 4]);
console.log(tracking.length, Array.from(tracking).join(","));
rab.resize(8);
console.log(rab.byteLength, tracking.length, Array.from(tracking).join(","));
tracking[7] = 77;
rab.resize(2);
console.log(rab.byteLength, tracking.length, Array.from(tracking).join(","));
rab.resize(8);
console.log(rab.byteLength, tracking.length, Array.from(tracking).join(","));
console.log("resize > max:", name(() => rab.resize(17)));
console.log("resize < 0:", name(() => rab.resize(-1)));
console.log("resize to max:", (rab.resize(16), rab.byteLength), tracking.length);
console.log("resize returns:", rab.resize(8));
rab.resize(8);
console.log("same-size resize:", rab.byteLength, tracking.length);

// --- fixed-length views go out of bounds and come back -----------------------
const rab2 = new ArrayBuffer(8, { maxByteLength: 16 });
const whole = new Uint8Array(rab2);
for (let i = 0; i < 8; i++) whole[i] = i + 1;
const fixedView = new Uint8Array(rab2, 4, 4); // bytes 4..8
const offsetTracking = new Uint8Array(rab2, 6); // bytes 6..end
console.log(fixedView.length, fixedView.byteOffset, fixedView.byteLength, Array.from(fixedView).join(","));
console.log(offsetTracking.length, offsetTracking.byteOffset, Array.from(offsetTracking).join(","));
rab2.resize(6);
console.log("shrunk 6:", fixedView.length, fixedView.byteOffset, fixedView.byteLength, fixedView[0]);
console.log("shrunk 6:", offsetTracking.length, offsetTracking.byteOffset);
rab2.resize(5);
console.log("shrunk 5:", offsetTracking.length, offsetTracking.byteOffset, offsetTracking[0]);
rab2.resize(12);
console.log("regrown:", fixedView.length, fixedView.byteOffset, Array.from(fixedView).join(","));
console.log("regrown:", offsetTracking.length, offsetTracking.byteOffset, Array.from(offsetTracking).join(","));
console.log("whole:", whole.length, Array.from(whole).join(","));

// --- multi-byte typed arrays -------------------------------------------------
const rab3 = new ArrayBuffer(8, { maxByteLength: 64 });
const i32 = new Int32Array(rab3);
const f64fixed = new Float64Array(rab3, 0, 1);
i32[0] = 0x01020304;
i32[1] = -1;
console.log(i32.length, i32.byteLength, f64fixed.length);
rab3.resize(18); // 4 whole int32 + 2 stray bytes: a tracking view floors
console.log(i32.length, i32.byteLength, i32[0], i32[1], i32[2], i32[3], i32[4]);
i32[3] = 123456;
rab3.resize(4);
console.log(i32.length, i32[0], i32[1], f64fixed.length, f64fixed.byteLength, f64fixed.byteOffset);
rab3.resize(16);
console.log(i32.length, i32[0], i32[1], i32[3], f64fixed.length);
const odd = new ArrayBuffer(6, { maxByteLength: 64 });
console.log("tracking over 6 bytes:", new Int32Array(odd).length);
console.log("fixed buffer of 6 bytes:", name(() => new Int32Array(new ArrayBuffer(6))));

// --- subarray: no `end` on a tracking view keeps tracking --------------------
const rab4 = new ArrayBuffer(8, { maxByteLength: 32 });
const base = new Uint8Array(rab4);
const tailTracking = base.subarray(2);
const tailFixed = base.subarray(2, 6);
const i16 = new Int16Array(rab4);
const i16Tracking = i16.subarray(1);
const i16Fixed = i16.subarray(1, 3);
console.log(tailTracking.length, tailFixed.length, i16Tracking.length, i16Fixed.length);
rab4.resize(16);
console.log(tailTracking.length, tailFixed.length, i16Tracking.length, i16Fixed.length);
rab4.resize(4);
console.log(tailTracking.length, tailFixed.length, i16Tracking.length, i16Fixed.length);

// --- DataView ----------------------------------------------------------------
const rab5 = new ArrayBuffer(8, { maxByteLength: 16 });
const dvTracking = new DataView(rab5);
const dvFixed = new DataView(rab5, 4, 4);
dvTracking.setUint32(4, 0xdeadbeef);
console.log(dvTracking.byteLength, dvFixed.byteLength, dvFixed.getUint32(0).toString(16));
rab5.resize(16);
console.log(dvTracking.byteLength, dvFixed.byteLength, dvTracking.getUint8(15));
rab5.resize(6);
console.log(dvTracking.byteLength, dvTracking.getUint8(5).toString(16));
console.log("dv read past end:", name(() => dvTracking.getUint8(6)));
console.log("oob dv byteLength:", name(() => dvFixed.byteLength));
console.log("oob dv get:", name(() => dvFixed.getUint8(0)));
console.log("oob dv set:", name(() => dvFixed.setUint8(0, 1)));
rab5.resize(8);
console.log("dv back:", dvFixed.byteLength, dvFixed.getUint32(0).toString(16));

// --- slice / transfer --------------------------------------------------------
const rab6 = new ArrayBuffer(4, { maxByteLength: 8 });
new Uint8Array(rab6).set([9, 8, 7, 6]);
const sliced = rab6.slice(1, 3);
console.log(sliced.resizable, sliced.byteLength, Array.from(new Uint8Array(sliced)).join(","));
const moved = rab6.transfer();
console.log(moved.resizable, moved.maxByteLength, moved.byteLength, rab6.detached, rab6.byteLength, rab6.maxByteLength);
moved.resize(8);
console.log(Array.from(new Uint8Array(moved)).join(","));
console.log("transfer > max:", name(() => moved.transfer(9)));
const pinned = moved.transferToFixedLength();
console.log(pinned.resizable, pinned.maxByteLength, pinned.byteLength, moved.detached);
console.log("resize detached:", name(() => moved.resize(1)));
console.log("resize pinned:", name(() => pinned.resize(1)));

// --- reflection --------------------------------------------------------------
console.log(typeof ArrayBuffer.prototype.resize, ArrayBuffer.prototype.resize.length, ArrayBuffer.prototype.resize.name);
console.log(typeof Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "resizable")?.get);
console.log(typeof Object.getOwnPropertyDescriptor(ArrayBuffer.prototype, "maxByteLength")?.get);
const viaProto = new ArrayBuffer(2, { maxByteLength: 4 });
ArrayBuffer.prototype.resize.call(viaProto, 3);
console.log(viaProto.byteLength);
console.log("resize.call(non-buffer):", name(() => ArrayBuffer.prototype.resize.call({} as any, 1)));
console.log(typeof (new Uint8Array(2) as any).resize, typeof (new DataView(new ArrayBuffer(2)) as any).resize);
const dynamicCtor: any = [ArrayBuffer][0];
const dyn = new dynamicCtor(1, { maxByteLength: 3 });
console.log(dyn.resizable, dyn.maxByteLength, dyn.byteLength);

// --- element reads in a loop that resizes underneath them --------------------
const rab7 = new ArrayBuffer(8, { maxByteLength: 8 });
const loopView = new Uint8Array(rab7);
for (let i = 0; i < 8; i++) loopView[i] = 10 + i;
let seen = "";
for (let i = 0; i < loopView.length; i++) {
  seen += loopView[i] + ";";
  if (i === 2) rab7.resize(5);
}
console.log(seen, loopView.length);
const f32 = new Float32Array(new ArrayBuffer(16, { maxByteLength: 16 }));
f32.fill(1.5);
let total = 0;
for (let i = 0; i < 4; i++) {
  const v = f32[i];
  total += v === undefined ? 100 : v;
  if (i === 1) (f32.buffer as ArrayBuffer).resize(8);
}
console.log(total, f32.length);

// --- the Native Messaging host shape (guest271314/NativeMessagingHosts) -------
const host = new ArrayBuffer(0, { maxByteLength: 1024 ** 2 * 64 });
let sum = 0;
for (const size of [1024, 1024 * 1024, 3, 0, 5 * 1024 * 1024]) {
  host.resize(size);
  const view = new Uint8Array(host);
  for (let i = 0; i < size; i += 4093) view[i] = i & 0xff;
  for (let i = 0; i < size; i += 4093) sum += view[i];
  console.log(size, view.length, view[0], size > 0 ? view[size - 1] : -1);
  host.resize(0);
  console.log(view.length, host.byteLength);
}
console.log(sum);
