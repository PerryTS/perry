// Numeric lane semantics on owning storage, offset and nested backing views.
function readI8(a: Int8Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('i8', a[keys[i]]); }
function readClamp(a: Uint8ClampedArray, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('clamp', a[keys[i]]); }
function readI16(a: Int16Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('i16', a[keys[i]]); }
function readU16(a: Uint16Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('u16', a[keys[i]]); }
function readI32(a: Int32Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('i32', a[keys[i]]); }
function readU32(a: Uint32Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('u32', a[keys[i]]); }
function readF16(a: Float16Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('f16', a[keys[i]], Object.is(a[keys[i]], -0)); }
function readF32(a: Float32Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('f32', a[keys[i]], Object.is(a[keys[i]], -0)); }
function readF64(a: Float64Array, keys: any[]) { for (let i=0;i<keys.length;i++) console.log('f64', a[keys[i]], Object.is(a[keys[i]], -0)); }
const keys: any[] = [0,1,2,3,4,-1,0.5,NaN,Infinity,'0','-0','01','1.0',4294967296];
const i8 = new Int8Array([-129,-128,127,256]); readI8(i8,keys); readI8(i8.subarray(1).subarray(1),keys);
const clamp = new Uint8ClampedArray([-1,2.5,3.5,999]); readClamp(clamp,keys); readClamp(clamp.subarray(1),keys);
const i16 = new Int16Array([-32769,-32768,32767,65536]); readI16(i16,keys); readI16(i16.subarray(1),keys);
const u16 = new Uint16Array([-1,32768,65535,65536]); readU16(u16,keys); readU16(u16.subarray(1),keys);
const i32 = new Int32Array([-2147483649,-2147483648,2147483647,4294967296]); readI32(i32,keys); readI32(i32.subarray(1),keys);
const u32 = new Uint32Array([-1,2147483648,4294967295,4294967296]); readU32(u32,keys); readU32(u32.subarray(1),keys);
const f16 = new Float16Array([-0,2.980232238769532e-8,1.00048828125,NaN]); readF16(f16,keys); readF16(f16.subarray(1),keys);
const f32 = new Float32Array([-0,1/3,Infinity,NaN]); readF32(f32,keys); readF32(f32.subarray(1),keys);
const f64 = new Float64Array([-0,1/3,-Infinity,NaN]); readF64(f64,keys); readF64(f64.subarray(1),keys);
// A lying annotation must recover ordinary elements, including object values.
readI32([17, 'x', null, undefined] as any, [0,1,2,3,4]);
readF64({0: 1.5, 1: 'v'} as any, [0,1,2]);
readI8(new Uint16Array([300,65535]) as any,[0,1,2]);
// Raw NaN payloads must never forge NaN-boxed pointers.
const raw = new Uint32Array(4); raw[0]=0xffffffff; raw[1]=0x7fffffff; raw[2]=0xffffffff; raw[3]=0x7ffdffff;
readF64(new Float64Array(raw.buffer),[0,1]); readF32(new Float32Array(raw.buffer),[0,1,2,3]);

readF64('ab' as any,[0,1,2]); readI32(42 as any,[0,1]);
