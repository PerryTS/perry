// The DataView half of the #7219 aliasing family (the typed-array-to-typed-array
// half is `test_gap_typedarray_buffer_aliasing_7219.ts`).
//
// A DataView owns a BufferHeader seeded from the backing ArrayBuffer at
// construction, and only writes routed through the buffer view registry
// refreshed that snapshot. A Uint16Array/Uint32Array/Float64Array element store
// goes straight into the backing store, so nothing refreshed the snapshot and
// every `get*` returned the bytes present when the DataView was built — no
// throw, no null, just stale numbers.
//
// A Uint8Array writer masked it (its element writes go through the registry and
// mirror into every view), which is why the bug read as "DataView is fine".

// The issue's reproduction. The sum is endian-independent: the four bytes are
// 1, 2, 3 and 4 in either order.
function u32Writer(): number {
  const ab = new ArrayBuffer(4);
  const words = new Uint32Array(ab);
  const dv = new DataView(ab);
  words[0] = 0x01020304;
  return dv.getUint8(0) + dv.getUint8(1) + dv.getUint8(2) + dv.getUint8(3);
}
console.log("u32 writer:", u32Writer());

// Every element width, including the one-byte case that always worked.
function everyWidth(): string {
  const parts: number[] = [];
  {
    const ab = new ArrayBuffer(4);
    const w = new Uint8Array(ab);
    const dv = new DataView(ab);
    w[0] = 1;
    w[3] = 4;
    parts.push(dv.getUint8(0) + dv.getUint8(3));
  }
  {
    const ab = new ArrayBuffer(4);
    const w = new Uint16Array(ab);
    const dv = new DataView(ab);
    w[0] = 0x0102;
    w[1] = 0x0304;
    parts.push(dv.getUint8(0) + dv.getUint8(1) + dv.getUint8(2) + dv.getUint8(3));
  }
  {
    const ab = new ArrayBuffer(4);
    const w = new Int32Array(ab);
    const dv = new DataView(ab);
    w[0] = -1;
    parts.push(dv.getUint8(0) + dv.getUint8(1) + dv.getUint8(2) + dv.getUint8(3));
  }
  {
    const ab = new ArrayBuffer(8);
    const w = new Float64Array(ab);
    const dv = new DataView(ab);
    w[0] = 1.5;
    let sum = 0;
    for (let i = 0; i < 8; i++) sum += dv.getUint8(i);
    parts.push(sum);
  }
  return parts.join(" ");
}
console.log("every width:", everyWidth());

// The reverse direction (DataView write, typed-array read) always worked
// because the DataView setter mirrors into the backing; it must stay working.
function dataViewWriter(): string {
  const ab = new ArrayBuffer(8);
  const words = new Uint32Array(ab);
  const doubles = new Float64Array(new ArrayBuffer(8));
  const dv = new DataView(ab);
  const dv2 = new DataView(doubles.buffer);
  dv.setUint32(0, 0x01020304, true);
  dv2.setFloat64(0, -3.25, true);
  return `${words[0]} ${doubles[0]}`;
}
console.log("DataView writer:", dataViewWriter());

// Constructing the DataView AFTER the write always worked — the snapshot
// captured the bytes already there — which is what made this look like a
// construction-order quirk rather than a lost alias. Writes after that point
// must keep flowing.
function constructedAfter(): string {
  const ab = new ArrayBuffer(4);
  const words = new Uint32Array(ab);
  words[0] = 0x01020304;
  const dv = new DataView(ab);
  const first = dv.getUint8(0) + dv.getUint8(1) + dv.getUint8(2) + dv.getUint8(3);
  words[0] = 0x05060708;
  const second = dv.getUint8(0) + dv.getUint8(1) + dv.getUint8(2) + dv.getUint8(3);
  return `${first} ${second}`;
}
console.log("constructed after:", constructedAfter());

// A windowed DataView reads its own [byteOffset, +byteLength) slice of the
// backing, and writes land there — the window must not shift by the resolution.
function windowedView(): string {
  const ab = new ArrayBuffer(16);
  const words = new Uint32Array(ab);
  const dv = new DataView(ab, 4, 8);
  words[0] = 0x11111111;
  words[1] = 0x01020304;
  words[2] = 0x05060708;
  words[3] = 0x22222222;
  let sum = 0;
  for (let i = 0; i < 8; i++) sum += dv.getUint8(i);
  dv.setUint8(0, 0xff);
  return `${sum} ${dv.byteLength} ${dv.byteOffset} ${words[1] !== 0x01020304}`;
}
console.log("windowed view:", windowedView());

// DataView is big-endian by default while a typed array is platform-endian, so
// the two disagree on purpose. Reading the SAME bytes both ways is what pins
// the fix to "alias the storage" rather than "byte-swap somewhere".
function endianness(): string {
  const ab = new ArrayBuffer(8);
  const bytes = new Uint8Array(ab);
  const dv = new DataView(ab);
  bytes[0] = 0x01;
  bytes[1] = 0x02;
  const i16 = `${dv.getInt16(0)} ${dv.getInt16(0, true)}`;
  const words = new Int32Array(ab);
  words[0] = -66052; // 0xFFFEFDFC
  const i32 = `${dv.getInt32(0)} ${dv.getInt32(0, true)}`;
  const doubles = new Float64Array(ab);
  doubles[0] = -3.25;
  const f64 = `${dv.getFloat64(0, true)}`;
  return `${i16} ${i32} ${f64}`;
}
console.log("endianness:", endianness());

// Two DataViews over one buffer, one of them offset: both must track the
// backing and each other.
function twoViews(): string {
  const ab = new ArrayBuffer(8);
  const words = new Uint16Array(ab);
  const whole = new DataView(ab);
  const tail = new DataView(ab, 2);
  words[1] = 0xbeef;
  const seen = `${whole.getUint16(2)} ${tail.getUint16(0)}`;
  tail.setUint16(0, 0x1234);
  return `${seen} ${words[1]} ${whole.getUint16(2)}`;
}
console.log("two views:", twoViews());

// Module scope, not inside a function: the same shapes must hold when the
// bindings are module-level (a different codegen tier).
const mAb = new ArrayBuffer(8);
const mWords = new Uint32Array(mAb);
const mDv = new DataView(mAb);
mWords[0] = 0x01020304;
mWords[1] = 0x05060708;
console.log(
  "module scope:",
  mDv.getUint8(0) + mDv.getUint8(1) + mDv.getUint8(2) + mDv.getUint8(3),
  mDv.getUint32(4, true) === 0x05060708 || mDv.getUint32(4) === 0x05060708,
);

// A DataView over a typed array's lazily-materialized `.buffer` aliases the
// same storage in both directions.
function overMaterializedBuffer(): string {
  const words = new Uint32Array(2);
  const dv = new DataView(words.buffer);
  words[0] = 0x01020304;
  const read = dv.getUint8(0) + dv.getUint8(1) + dv.getUint8(2) + dv.getUint8(3);
  dv.setUint8(4, 1);
  dv.setUint8(5, 2);
  dv.setUint8(6, 3);
  dv.setUint8(7, 4);
  return `${read} ${words[1] !== 0}`;
}
console.log("over materialized buffer:", overMaterializedBuffer());

// A loop that writes through the typed array and reads through the DataView —
// the shape any codegen fast path would take over.
function loopedMix(): string {
  const ab = new ArrayBuffer(64);
  const words = new Uint32Array(ab);
  const dv = new DataView(ab);
  for (let i = 0; i < 16; i++) words[i] = i * 0x01010101;
  let sum = 0;
  for (let i = 0; i < 64; i++) sum += dv.getUint8(i);
  for (let i = 0; i < 16; i++) dv.setUint32(i * 4, i + 1, true);
  let total = 0;
  for (let i = 0; i < 16; i++) total += words[i];
  return `${sum} ${total}`;
}
console.log("looped mix:", loopedMix());

// Buffer identity and metadata were already right — only the bytes were not
// shared, which is why this reads as a correctness bug, not a modelling one.
function identity(): string {
  const ab = new ArrayBuffer(16);
  const words = new Uint32Array(ab, 4);
  const dv = new DataView(ab, 4, 8);
  return `${words.buffer === ab} ${dv.buffer === ab} ${dv.byteLength} ${dv.byteOffset} ${words.byteOffset}`;
}
console.log("identity:", identity());
