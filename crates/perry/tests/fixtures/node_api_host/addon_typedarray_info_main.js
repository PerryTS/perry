// napi_get_typedarray_info reports a typed array's own type, length, byte
// offset and data pointer, and its ArrayBuffer only when asked, exactly as
// Node does for plain, offset, Buffer, detached and out-of-bounds views.
const addon = require("fixture-typedarray-info");

function show(label, value) {
  console.log(label, addon.info(value));
}
function same(label, value) {
  console.log(label, "buffer identity", addon.buffer(value) === value.buffer);
}

const plain = new Uint8Array(16);
plain[0] = 11;
plain[3] = 33;
show("Uint8Array(16)", plain);
same("Uint8Array(16)", plain);

const ab = new ArrayBuffer(32);
new Uint8Array(ab)[4] = 44;
new Uint8Array(ab)[8] = 88;
const u16 = new Uint16Array(ab, 4, 3);
show("Uint16Array(ab,4,3)", u16);
same("Uint16Array(ab,4,3)", u16);
const f64 = new Float64Array(ab, 8, 2);
show("Float64Array(ab,8,2)", f64);
const i32 = new Int32Array([7, 8, 9]);
show("Int32Array([7,8,9])", i32);
const sub = i32.subarray(1);
show("Int32Array.subarray(1)", sub);
same("Int32Array.subarray(1)", sub);
const big = new BigInt64Array(2);
show("BigInt64Array(2)", big);
const clamped = new Uint8ClampedArray([200, 1]);
show("Uint8ClampedArray", clamped);

const nb = Buffer.alloc(8, 5);
show("Buffer.alloc(8)", nb);
same("Buffer.alloc(8)", nb);
const nbsub = nb.subarray(2, 5);
show("Buffer.subarray(2,5)", nbsub);
const u8sub = plain.subarray(3, 9);
show("Uint8Array.subarray(3,9)", u8sub);

const det = new ArrayBuffer(16);
const detView = new Uint8Array(det, 4, 8);
const detWhole = new Int16Array(det);
det.transfer();
show("detached view", detView);
show("detached whole", detWhole);
same("detached view", detView);
const own = new Float32Array(4);
own.buffer.transfer();
show("detached own", own);

const rab = new ArrayBuffer(16, { maxByteLength: 32 });
new Uint8Array(rab)[2] = 22;
const fixed = new Uint8Array(rab, 8, 4);
const tracking = new Uint8Array(rab, 2);
rab.resize(4);
show("out-of-bounds fixed", fixed);
show("tracking after shrink", tracking);
rab.resize(32);
show("fixed after grow", fixed);
show("tracking after grow", tracking);

show("DataView", new DataView(new ArrayBuffer(4)));
show("ArrayBuffer", new ArrayBuffer(4));
show("object", {});
show("number", 1);

for (let type = 0; type <= 12; type++) {
  const made = addon.create(new ArrayBuffer(16), type);
  console.log("create", type, typeof made === "number" ? "status=" + made : made.constructor.name + " " + made.length);
}
