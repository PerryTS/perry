// An integer local used as an index holds exactly its JS value: an init or a
// step whose exact result leaves the int32 range is that exact value, never
// its low 32 bits.

function mulInit(i: number, n: number, marks: Uint8Array): number {
  let steps = 0;
  for (let j = i * i; j <= n; j += i) {
    marks[j] = 1;
    steps++;
  }
  return steps;
}

function addInit(a: number, b: number, n: number, marks: Int32Array): number {
  let steps = 0;
  for (let j = a + b; j <= n; j++) {
    marks[j] = 7;
    steps++;
  }
  return steps;
}

function subInit(a: number, b: number, marks: Float64Array): string {
  const j = a - b;
  marks[j] = 1.5;
  return j + " " + marks[j];
}

function stepPastMax(start: number, step: number, n: number, marks: Uint8Array): number {
  let steps = 0;
  for (let j = start; j <= n; j += step) {
    marks[j] = 1;
    steps++;
  }
  return steps;
}

function incrementPastMax(marks: Uint8Array): number {
  let k = 2147483640;
  for (let s = 0; s < 20; s++) {
    marks[k] = 1;
    k++;
  }
  return k;
}

function upToMax(n: number, marks: Uint8Array): number {
  let c = 0;
  for (let i = 2147483640; i <= n; i++) {
    marks[i & 7] = 1;
    c++;
  }
  return c;
}

function readSide(i: number, n: number, marks: Uint8Array): number {
  let sum = 0;
  for (let j = i * i; j <= n; j += i) sum += marks[j];
  return sum;
}

function sieve(n: number): number {
  const marks = new Uint8Array(n + 1);
  let count = 0;
  for (let i = 2; i <= n; i++) {
    if (marks[i] === 0) {
      count++;
      for (let j = i * i; j <= n; j += i) marks[j] = 1;
    }
  }
  return count;
}

function nonZero(a: ArrayLike<number>): number {
  let c = 0;
  for (let k = 0; k < a.length; k++) if (a[k] !== 0) c++;
  return c;
}

const u8 = new Uint8Array(50001);
console.log("mulInit", mulInit(46349, 50000, u8), nonZero(u8));
console.log("mulInit small", mulInit(7, 100, new Uint8Array(101)));
const i32 = new Int32Array(64);
console.log("addInit", addInit(2147483640, 20, -2147483600, i32), nonZero(i32));
console.log("addInit small", addInit(3, 4, 20, new Int32Array(21)));
const f64 = new Float64Array(64);
console.log("subInit", subInit(-2147483640, 20, f64), nonZero(f64));
console.log("subInit small", subInit(9, 4, new Float64Array(8)));
const u8b = new Uint8Array(64);
console.log("stepPastMax", stepPastMax(2147000000, 1000000, 2147483647, u8b), nonZero(u8b));
console.log("stepPastMax small", stepPastMax(1, 5, 30, new Uint8Array(31)));
const u8c = new Uint8Array(64);
console.log("incrementPastMax", incrementPastMax(u8c), nonZero(u8c));
console.log("upToMax", upToMax(2147483647, new Uint8Array(8)), upToMax(2147483645, new Uint8Array(8)));
const ones = new Uint8Array(50001).fill(1);
console.log("readSide", readSide(46349, 50000, ones), readSide(100, 50000, ones));
console.log("sieve", sieve(100000), sieve(1000));
