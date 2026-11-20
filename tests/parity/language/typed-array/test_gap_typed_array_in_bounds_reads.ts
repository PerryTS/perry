// Reads of a typed array born with a known length (`new Float32Array([...])`,
// `new Float64Array(n)`), copied into a program Array: in bounds they are
// Numbers, past the end, at a negative or fractional index, or after the
// buffer is detached they are `undefined`.
class M4 {
  elements: any;
  constructor() {
    this.elements = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1];
  }
  fromArray(array: any, offset = 0) {
    for (let i = 0; i < 16; i++) this.elements[i] = array[i + offset];
    return this;
  }
}

const src = new Float32Array([1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0.5, 0.25, 0.125, 1]);
const m = new M4().fromArray(src);
console.log(m.elements.join(","));

const sized = new Float64Array(16);
for (let i = 0; i < 16; i++) sized[i] = i * 1.5;
console.log(new M4().fromArray(sized).elements.join(","));

// Past the end: the last four reads are undefined.
const past = new M4().fromArray(new Float32Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]));
console.log(past.elements.map((v: any) => String(v)).join(","));

// A negative offset: the first read is undefined.
const neg = new M4().fromArray(new Float32Array(16), -1);
console.log(String(neg.elements[0]), String(neg.elements[1]));

// A fractional offset reads no element at all.
const frac = new M4().fromArray(new Float32Array(32), 0.5);
console.log(String(frac.elements[0]), String(frac.elements[15]));

// A detached buffer: every read is undefined.
const det = new Float32Array([1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16]);
const view: any = det.subarray(0);
(view.buffer as any).transfer();
const d = new M4().fromArray(det);
console.log(det.length, String(d.elements[0]), String(d.elements[15]));

let sum = 0;
for (let k = 0; k < 4; k++) sum += m.elements[k * 5];
console.log(sum);
