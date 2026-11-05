// B4 reads data and length from the owner header for either placement.
function inspectFloat(a: Float64Array): number {
  a[a.length - 1] = 91.25;
  return a[0] + a[a.length - 1];
}
function inspectInt(a: Int32Array): number {
  a[0] = -1234567;
  a[a.length - 1] = 987654;
  return a[0] + a[a.length - 1];
}
function inspectByte(a: Uint8Array): number {
  a[0] = 37;
  a[a.length - 1] = 91;
  return a[0] + a[a.length - 1];
}
// Allocation-bearing back edges exercise moving-GC polls in forced runs.
function keepOwnersLive(a: Float64Array, b: Int32Array, c: Uint8Array): void {
  const roots: any[] = [];
  for (let n = 0; n < 64; n++) {
    roots[n & 7] = { n, a, b, c };
    if (a.length !== 8192 || b.length !== 8192 || c.length !== 32768)
      throw new Error('Native owner length changed across collection');
  }
  if (roots[7].n !== 63 || roots[7].a !== a)
    throw new Error('Native owner root did not survive collection');
}
const f = new Float64Array(8192);
const i = new Int32Array(8192);
const u = new Uint8Array(32768);
keepOwnersLive(f, i, u);
console.log('float', f.length, f.byteLength, inspectFloat(f), f.slice(-1)[0]);
console.log('int', i.length, i.byteLength, inspectInt(i), i.subarray(1).length);
console.log('byte', u.length, u.byteLength, inspectByte(u), u.subarray(1).length);
const fs = f.subarray(1);
const moved = structuredClone({f, i, u}, {transfer: [f.buffer, i.buffer, u.buffer]});
console.log('detached', f.length, f.byteLength, fs.length, i.length, i.byteLength, u.length, u.byteLength);
keepOwnersLive(moved.f, moved.i, moved.u);
console.log('moved', moved.f.length, moved.i.length, moved.u.length, inspectFloat(moved.f), inspectInt(moved.i), inspectByte(moved.u));
const direct = moved.u.buffer.transfer();
console.log('transfer', moved.u.length, moved.u.byteLength, direct.byteLength, new Uint8Array(direct)[32767]);

// Cloned owners must not acquire Buffer pool provenance before rebranding.
const small = structuredClone(new Uint8Array([1, 2, 3]));
const smallAB = structuredClone(new ArrayBuffer(3));
console.log('small clone', small.length, small.buffer.byteLength, smallAB.byteLength);
const smallMoved = smallAB.transfer();
console.log('small independent', smallMoved.byteLength, small.length, small[2]);
