// A field's declared typed-array kind only selects the inline element access;
// the value's own owner brand must prove it. Each field below holds an array
// of ANOTHER kind (and of a neighbouring brand), a view, or no typed array,
// so every access must follow the actual value, never the annotation.
class Lanes {
  i8: Int8Array;
  u8: Uint8Array;
  i16: Int16Array;
  i32: Int32Array;
  u16: Uint16Array;
  clamp: Uint8ClampedArray;
  u32: Uint32Array;
  f32: Float32Array;
  f64: Float64Array;
  constructor() {
    this.i8 = new Uint8Array(4) as any;
    this.u8 = new Int8Array(4) as any;
    this.i16 = new Int32Array(4) as any;
    this.i32 = new Int16Array(4) as any;
    this.u16 = new Uint8ClampedArray(4) as any;
    this.clamp = new Uint16Array(4) as any;
    this.u32 = new Float64Array(4) as any;
    this.f32 = new BigInt64Array(4) as any;
    this.f64 = new Int32Array(4) as any;
  }
}

function write(l: Lanes, i: number, v: number) {
  l.i8[i] = v;
  l.u8[i] = v;
  l.i16[i] = v;
  l.i32[i] = v;
  l.u16[i] = v;
  l.clamp[i] = v;
  l.u32[i] = v;
  l.f64[i] = v;
}

function read(l: Lanes, i: number): string {
  return [l.i8[i], l.u8[i], l.i16[i], l.i32[i], l.u16[i], l.clamp[i], l.u32[i], l.f64[i]].join(',');
}

const l = new Lanes();
for (const v of [-1, 70000, 3.75, -129.5, 2 ** 31 + 5, 300]) {
  write(l, 1, v);
  console.log(v, read(l, 1), read(l, 0), l.i32[1] < 0, l.i8[1] < 0);
}
try {
  l.f32[2] = 5 as any;
  console.log('bigint-store', String(l.f32[2]));
} catch (e) {
  console.log('bigint-store throws', (e as Error).name);
}
l.f32[3] = 7n as any;
console.log('bigint', String(l.f32[3]), typeof l.f32[3]);

// Views and offset views of the declared kind are not owners.
const backing = new ArrayBuffer(32);
l.i32 = new Int32Array(backing, 8, 4);
l.f64 = new Float64Array(backing, 0, 2) as any;
l.i32[0] = 77;
console.log('views', l.i32[0], new Int32Array(backing)[2], l.f64[1]);
l.i32 = new Int16Array(backing) as any;
console.log('alias', l.i32[4], l.i32[5]);
