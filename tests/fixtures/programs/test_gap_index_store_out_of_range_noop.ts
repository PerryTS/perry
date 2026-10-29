// A store at an index outside [0, length) changes nothing: typed arrays drop
// it, and an index computed past the int32 range is that exact index, not its
// low 32 bits.

function stride(i: number, n: number, marks: Uint8Array): number {
  let steps = 0;
  for (let j = i * i; j <= n; j += i) {
    marks[j] = 1;
    steps++;
  }
  return steps;
}

function directProduct(i: number, marks: Uint8Array): number {
  marks[i * i] = 1;
  return marks[i * i] === undefined ? -1 : marks[i * i];
}

function directSum(a: number, b: number, marks: Int32Array): number {
  marks[a + b] = 5;
  return marks.length;
}

function arrayStride(i: number, n: number, arr: number[]): number {
  let steps = 0;
  for (let j = i * i; j <= n; j += i) {
    arr[j] = 1;
    steps++;
  }
  return steps;
}

function nonZero(a: ArrayLike<number>): number {
  let c = 0;
  for (let k = 0; k < a.length; k++) if (a[k] !== 0) c++;
  return c;
}

let total = 0;
for (let r = 0; r < 3; r++) {
  const marks = new Uint8Array(200001);
  for (let i = 2; i <= 200000; i++) total += stride(i, 200000, marks);
  total += nonZero(marks);
}
console.log("stride", total);

const big = new Uint8Array(200000);
console.log("directProduct", directProduct(65537, big), nonZero(big));
const i32 = new Int32Array(16);
console.log("directSum", directSum(2147483640, 20, i32), nonZero(i32));

const arr: number[] = new Array(50001).fill(0);
console.log("arrayStride", arrayStride(46349, 50000, arr), nonZero(arr), arr.length);
