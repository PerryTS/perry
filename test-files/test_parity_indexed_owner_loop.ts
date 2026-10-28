// Generic indexed-loop admission: numeric owners, aliases and exits.
function fresh(n: number): Float64Array { return new Float64Array(n); }
function add(dst: Float64Array, src: Float64Array, off: number, n: number, scale: number) {
  for (let i = 0; i < n; i++) { dst[off + i] += src[i] * scale; }
  return dst[off];
}
const x = fresh(8); x[0] = 2; x[1] = 3; x[2] = 5;
console.log('fresh', add(x, x, 1, 3, 2), x[2], x[3]);
console.log('fraction', add(x, x, .5, 2, 1), x[0]);
console.log('negative', add(x, x, -1, 2, 1));
console.log('nan', add(x, x, NaN, 2, 1));
console.log('oob', add(x, x, 100, 2, 1));
console.log('zero', add(x, x, 0, 0, NaN));
const view = new Float64Array(x.buffer, 8, 3);
console.log('view', add(view, view, 0, 2, 1));
const fake = {0: 5, 1: 7, length: 2} as unknown as Float64Array;
console.log('lie', add(fake, fake, 0, 2, 2));
let effects = 0;
const scale = {valueOf() { effects++; return 2; }} as unknown as number;
console.log('coerce', add(x, x, 0, 2, scale), effects);
function rhs(a: Float64Array) {
  for (let i = 0; i < 2; i++) a[i] += (() => { a[i] = 20; return 3; })();
}
rhs(x); console.log('rhs', x[0], x[1]);
function scan(data: Uint8Array, table: Int32Array) {
  let acc = -1;
  for (let i = 0; i < data.length; i++) acc = table[(acc ^ data[i]) & 255] ^ (acc >>> 8);
  return acc;
}
const table = new Int32Array(256); for (let i = 0; i < 256; i++) table[i] = i * 37;
console.log('scan', scan(new Uint8Array([1, 2, 255, 7]), table));
// Different widths all use the same operation-region mechanism.
function widths(a: Int8Array, b: Uint16Array, c: Int32Array, d: Uint32Array, f: Float32Array) {
  for (let i = 0; i < 4; i++) { a[i] = b[i] + 1; c[i] = d[i] ^ a[i]; f[i] = c[i] / 3; }
}
const a = new Int8Array(4), b = new Uint16Array([255, 65535, 32768, 7]);
const c = new Int32Array(4), d = new Uint32Array([0xffffffff, 0x80000000, 1, 0]);
const f = new Float32Array(4); widths(a, b, c, d, f);
console.log('widths', a[0], a[1], c[0], c[1], f[0], f[3]);
function whole(a: Float64Array, x: number) {
  for (let i = 0; i < 2; i++) a[i] = x;
}
whole(x, -0); console.log('minuszero', Object.is(x[0], -0));
whole(x, NaN); console.log('nanvalue', Number.isNaN(x[0]));
whole(x, Infinity); console.log('inf', x[0]);
// Floating lanes may contain NaN-box-shaped payloads: they remain Numbers.
const rawFloats = fresh(4);
const rawBits = new BigUint64Array(rawFloats.buffer);
rawBits[0] = 0x7ffc000000000001n;
rawBits[1] = 0xfffc000000000010n;
rawBits[2] = 0x7ff0000000000000n;
rawBits[3] = 0x8000000000000000n;
const rawOut = new Float64Array(4);
function copyRaw(a: Float64Array, b: Float64Array) {
  for (let i = 0; i < 4; i++) b[i] = a[i];
}
copyRaw(rawFloats, rawOut);
console.log('raw', Number.isNaN(rawOut[0]), Number.isNaN(rawOut[1]), rawOut[2], Object.is(rawOut[3], -0));
function narrowed(a: Int32Array, v: number) {
  for (let i = 0; i < 2; i++) a[i] = v;
}
const ints = new Int32Array(2);
for (const v of [NaN, Infinity, -Infinity, 1e100, -1e100, 4294967297.75, -4294967297.75]) {
  narrowed(ints, v); console.log('narrow', ints[0]);
}
function sumBeforeIntegerWrite(a: Float64Array, v: number) {
  for (let i = 0; i < 2; i++) { a[i] = v; v = v | 0; }
}
sumBeforeIntegerWrite(rawOut, -0); console.log('initial-zero', Object.is(rawOut[0], -0), Object.is(rawOut[1], -0));
const moved = fresh(32);
let retained = new Float64Array(1);
for (let round = 0; round < 50; round++) {
  retained = new Float64Array(4096);
  for (let i = 0; i < 32; i++) moved[i] = moved[i] + 1;
}
console.log('relocated', moved[0], moved[31], retained.length);

function readClamp(a: Uint8ClampedArray) {
  let total = 0; for (let i = 0; i < 4; i++) total = total + a[i]; return total;
}
function readHalf(a: Float16Array) {
  let total = 0; for (let i = 0; i < 4; i++) total = total + a[i]; return total;
}
console.log('readwidths', readClamp(new Uint8ClampedArray([0, 1, 254, 255])), readHalf(new Float16Array([.5, 1.5, 2, 4])));

let lengthCalls = 0;
const getterArray = {0: 2, 1: 5, get length() { lengthCalls++; return 2; }} as unknown as Float64Array;
function getterLoop(a: Float64Array) {
  let total = 0; for (let i = 0; i < a.length; i++) total = total + a[i] + a[i]; return total;
}
console.log('getter', getterLoop(getterArray), lengthCalls);

function pingPong(a: Float64Array, b: Float64Array) {
  for (let round = 0; round < 6; round++) {
    const dst: Float64Array = (round & 1) ? a : b;
    const src: Float64Array = (round & 1) ? b : a;
    for (let i = 0; i < 4; i++) dst[i] += src[i] * 2;
  }
}
const ping = new Float64Array([1, 2, 3, 4]);
const pong = new Float64Array([5, 6, 7, 8]);
pingPong(ping, pong);
console.log('pingpong', ping[0], ping[3], pong[0], pong[3]);

function sumBytes(a: Uint8Array) {
  let sum = 0; for (let i = 0; i < a.length; i++) sum = sum + a[i]; return sum;
}
let ownLengthReads = 0;
const shadowOwner = new Uint8Array([3, 7, 11]);
Object.defineProperty(shadowOwner, 'length', {get() { ownLengthReads++; return 2; }});
console.log('shadow-length', sumBytes(shadowOwner), ownLengthReads);
let prototypeLengthReads = 0;
Object.defineProperty(Uint8Array.prototype, 'length', {configurable: true, get() { prototypeLengthReads++; return 1; }});
const protoSum = sumBytes(new Uint8Array([5, 9, 13]));
const protoReads = prototypeLengthReads;
delete (Uint8Array.prototype as any).length;
console.log('proto-length', protoSum, protoReads);
