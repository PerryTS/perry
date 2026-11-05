// #3657 DataView constructor: argument validation throws + correct
// byteOffset/byteLength/buffer accessors (incl. zero-length edge cases).

function check(label: string, fn: () => void, expected: string) {
  try {
    fn();
    console.log(label, "NO THROW (expected " + expected + ")");
  } catch (e: any) {
    console.log(label, e.constructor.name === expected ? "ok" : "got " + e.constructor.name);
  }
}

const ab = new ArrayBuffer(8);

// ---- validation throws ----
check("neg-offset", () => { new DataView(ab, -1); }, "RangeError");
check("neg-offset-inf", () => { new DataView(ab, -Infinity); }, "RangeError");
check("excessive-offset", () => { new DataView(ab, 9); }, "RangeError");
check("excessive-offset-inf", () => { new DataView(ab, Infinity); }, "RangeError");
check("neg-length", () => { new DataView(ab, 0, -1); }, "RangeError");
check("excessive-length", () => { new DataView(ab, 0, 9); }, "RangeError");
check("offset-plus-len", () => { new DataView(ab, 4, 5); }, "RangeError");
check("buffer-number", () => { new DataView(0 as any); }, "TypeError");
check("buffer-string", () => { new DataView("buffer" as any); }, "TypeError");
check("buffer-bool", () => { new DataView(true as any); }, "TypeError");
check("buffer-nan", () => { new DataView(NaN as any); }, "TypeError");

// ---- accessor values ----
const b3 = new ArrayBuffer(3);
const s1 = new DataView(b3, 1, 2);
console.log("s1", s1.byteOffset, s1.byteLength, s1.buffer === b3);
const s2 = new DataView(b3, 3, 0); // offset == length, zero-length view
console.log("s2", s2.byteOffset, s2.byteLength, s2.buffer === b3);
const s3 = new DataView(b3, 3); // implicit zero length at the end
console.log("s3", s3.byteOffset, s3.byteLength, s3.buffer === b3);

const b4 = new ArrayBuffer(4);
const s4 = new DataView(b4, 4); // offset == byteLength
console.log("s4", s4.byteOffset, s4.byteLength, s4.buffer === b4);
const s5 = new DataView(b4, 2, undefined); // explicit undefined length
console.log("s5", s5.byteOffset, s5.byteLength, s5.buffer === b4);
const s6 = new DataView(b4); // full-buffer view
console.log("s6", s6.byteOffset, s6.byteLength, s6.buffer === b4);

// ---- constructor / extensibility ----
console.log("ctor", s1.constructor === DataView);
console.log("extensible", Object.isExtensible(s1));

// reads/writes still index relative to the view start
const dv = new DataView(b4, 1, 2);
dv.setUint8(0, 0xab);
dv.setUint8(1, 0xcd);
const whole = new Uint8Array(b4);
console.log("aliased", whole[1].toString(16), whole[2].toString(16));
console.log("read-back", dv.getUint8(0).toString(16), dv.getUint8(1).toString(16));
