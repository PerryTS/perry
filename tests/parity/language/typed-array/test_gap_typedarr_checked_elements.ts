import { Buffer } from 'node:buffer';
class Cells {
  i8: Int8Array = new Int8Array(3);
  u8: Uint8Array = new Uint8Array(3);
  i16: Int16Array = new Int16Array(3);
  u16: Uint16Array = new Uint16Array(3);
  i32: Int32Array = new Int32Array(3);
  u32: Uint32Array = new Uint32Array(3);
  f32: Float32Array = new Float32Array(3);
  f64: Float64Array = new Float64Array(3);
  clamp: Uint8ClampedArray = new Uint8ClampedArray(3);
  bi: BigInt64Array = new BigInt64Array(3);
  bu: BigUint64Array = new BigUint64Array(3);
  buffer: Buffer = Buffer.alloc(3);
}
const c = new Cells();
c.i8[0] = -129; c.u8[0] = -1;
c.i16[0] = -32769; c.u16[0] = -1;
c.i32[0] = 4294967295; c.u32[0] = -1;
c.f32[0] = 1.1; c.f64[0] = -1.25;
c.clamp[0] = 0.5; c.clamp[1] = 1.5; c.clamp[2] = 254.5;
c.bi[0] = -1n; c.bu[0] = -1n; c.buffer[0] = -1;
console.log(c.i8[0], c.u8[0], c.i16[0], c.u16[0], c.i32[0], c.u32[0]);
console.log(c.f32[0], c.f64[0], c.clamp[0], c.clamp[1], c.clamp[2]);
console.log(String(c.bi[0]), String(c.bu[0]), c.buffer[0]);
c.i32[1] = NaN; c.u32[1] = Infinity; c.i8[1] = 1e21;
c.clamp[0] = -1; c.clamp[1] = NaN; c.clamp[2] = 999;
console.log(c.i32[1], c.u32[1], c.i8[1], c.clamp[0], c.clamp[1], c.clamp[2]);
c.i32[-1] = 44; c.i32[3] = 45; c.i32[NaN] = 46; c.i32[1.5] = 47;
console.log(c.i32[-1], c.i32[3], c.i32[NaN], c.i32[1.5], c.i32[0] < 0);
let effects = 0;
c.i32[99] = { valueOf() { effects++; return 2; } } as any;
console.log('coerce-oob', effects);
try { c.bi[99] = 1 as any; console.log('bad-bigint-coercion'); }
catch { console.log('bigint-coercion-throws'); }
function claimed(a: Int32Array) { console.log(a[0], a[0] < 0); a[0] = 19; console.log(a[0]); }
claimed([-2] as any);
claimed({0: '-3'} as any);
function containerClaim(h: Cells) {
  const v = h.i32[0];
  console.log('container', v, v < 0, v + 1);
}
containerClaim({i32: [-3]} as any);
containerClaim({i32: {0: '-3'}} as any);
const backing = new ArrayBuffer(16);
const aliased = new Int32Array(backing);
const bytes = new Uint8Array(backing);
aliased[0] = 257;
console.log('alias', bytes[0], bytes[1], aliased[0]);
const sab = new SharedArrayBuffer(16);
const shared = new Int32Array(sab);
shared[0] = -12;
console.log('shared', shared[0], aliased[0]);
const rab = new ArrayBuffer(16, {maxByteLength: 32});
const tracking = new Int32Array(rab);
const fixed = new Int32Array(rab, 4, 2);
tracking[0] = 5; fixed[0] = 6;
console.log('resize-before', tracking.length, fixed.length, tracking[1]);
rab.resize(4);
console.log('shrink', tracking.length, fixed.length, tracking[1], fixed[0]);
fixed[0] = 99; tracking[1] = 99;
rab.resize(24);
tracking[5] = 7;
console.log('grow', tracking.length, fixed.length, tracking[1], tracking[5]);
const detached = new Int32Array(2);
detached[0] = -8;
detached.buffer.transfer();
detached[0] = 9;
console.log('detach', detached.length, detached[0], detached[0] < 0);
