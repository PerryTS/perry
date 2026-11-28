// Every typed-array constructor makes a typed array: from a length, an array, an
// ArrayBuffer (whole, from an offset, and an offset with a length) and another typed
// array, in a plain and an async body. Each one's length, byteLength, byteOffset,
// elements, element writes, buffer and iteration match Node. On `--target web` every
// constructor but Uint8Array once made undefined, and a Uint8Array's length read 0
// (#15428). BigInt elements are left out: a bigint value is a separate gap on the web.

function show(label: string, t: any): string {
  const parts: string[] = [];
  for (let i = 0; i < t.length; i++) parts.push(String(t[i]));
  return [label, typeof t, t.length, t.byteLength, t.byteOffset, parts.join(",")].join(" ");
}

function cases(tag: string): string[] {
  const ab = new ArrayBuffer(16);
  const w = new Int32Array(3);
  w[0] = 10;
  w[1] = 20.7;
  w[2] = -1;
  const f = new Float32Array([3, 1, 2]);
  let sum = 0;
  for (const v of f) sum += v;
  const u = new Uint8Array([7, 8]);
  u[1] = 260;
  return [
    show(tag + " i8", new Int8Array([1, -2, 300])),
    show(tag + " u8", new Uint8Array([7, 8, 300])),
    show(tag + " u8c", new Uint8ClampedArray([7, -8, 300])),
    show(tag + " i16", new Int16Array([1, -2, 70000])),
    show(tag + " u16", new Uint16Array([1, -2])),
    show(tag + " i32", new Int32Array([1, 2, 1])),
    show(tag + " u32", new Uint32Array([1, -1])),
    show(tag + " f32", new Float32Array([1.5, 0.1])),
    show(tag + " f64", new Float64Array([1.5, 0.1])),
    show(tag + " len-i32", new Int32Array(4)),
    show(tag + " len-u8", new Uint8Array(3)),
    show(tag + " empty", new Int16Array()),
    show(tag + " ab", new Int32Array(ab)),
    show(tag + " ab-off-len", new Int32Array(ab, 4, 2)),
    show(tag + " u8-ab-off", new Uint8Array(ab, 8)),
    show(tag + " from-typed", new Float64Array(new Int16Array([3, 4]))),
    show(tag + " write", w),
    show(tag + " u8-write", u),
    [tag, "bigint", new BigInt64Array(2).length, new BigUint64Array(3).byteLength].join(" "),
    [tag, "buffer", w.buffer instanceof ArrayBuffer, w.buffer.byteLength, ab.byteLength].join(" "),
    [tag, "iter", sum, new Int32Array(1) instanceof Int32Array].join(" "),
  ];
}

console.log(cases("sync").join("\n"));

async function main(): Promise<void> {
  const ab = new ArrayBuffer(8);
  const t = new Int32Array([5, 6]);
  const u = new Uint8Array([9]);
  u[0] = 3;
  console.log(show("async i32", t));
  console.log(show("async u8", u));
  console.log(show("async f64", new Float64Array(1)));
  console.log(show("async ab", new Int16Array(ab, 2, 2)));
  console.log(["async", "length", t.length, u.length, ab.byteLength].join(" "));
}
main();
