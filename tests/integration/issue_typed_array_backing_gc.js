// A typed array's backing ArrayBuffer is materialized lazily and recorded only
// in `TYPED_ARRAY_VIEW_META`, a side table the collector did not trace. The
// buffer was therefore swept out from under a live typed array, and `subarray`
// — which builds its view from that backing — silently fell back to
// `js_typed_array_new`, reinterpreting the dead buffer's *byte* length as an
// element count: `subarray(0, 11)` on an `Int32Array(17)` returned a 68-element
// array (17 x 4), and a subsequent `set` threw "offset is out of bounds".

const a = new Int32Array(17);

if (a.subarray(0, 11).length !== 11) {
  throw new Error("subarray is broken before any collection");
}

for (let round = 0; round < 5; round++) {
  if (typeof gc === "function") {
    gc();
    gc();
  }
  for (let i = 0; i < 5000; i++) {
    const junk = { x: i, y: [i] };
  }
  if (typeof gc === "function") gc();

  const len = a.subarray(0, 11).length;
  if (len !== 11) {
    throw new Error(`round ${round}: subarray(0, 11).length = ${len}, expected 11`);
  }

  const buffer = a.buffer;
  if (!buffer || buffer.byteLength !== 68) {
    throw new Error(
      `round ${round}: a.buffer.byteLength = ${buffer && buffer.byteLength}, expected 68`,
    );
  }

  // The view must still alias the array, not a fresh copy.
  const view = a.subarray(0, 4);
  view[0] = 4242;
  if (a[0] !== 4242) {
    throw new Error(`round ${round}: subarray stopped aliasing (a[0] = ${a[0]})`);
  }
  a[0] = 0;
}

console.log("typed-array backing survived GC");
