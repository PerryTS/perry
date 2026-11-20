// Length-bound loop regions: compare byte-for-byte with the pinned Node.
const sizes: number[] = [1, 2, 3, 4];

function checkInt8Array(): void {
  const a = new Int8Array(sizes.length * 2);
  const b = new Int8Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Int8Array", own, inclusive, offset, cross, bad);
}
checkInt8Array();

function checkUint8Array(): void {
  const a = new Uint8Array(sizes.length * 2);
  const b = new Uint8Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Uint8Array", own, inclusive, offset, cross, bad);
}
checkUint8Array();

function checkUint8ClampedArray(): void {
  const a = new Uint8ClampedArray(sizes.length * 2);
  const b = new Uint8ClampedArray(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Uint8ClampedArray", own, inclusive, offset, cross, bad);
}
checkUint8ClampedArray();

function checkInt16Array(): void {
  const a = new Int16Array(sizes.length * 2);
  const b = new Int16Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Int16Array", own, inclusive, offset, cross, bad);
}
checkInt16Array();

function checkUint16Array(): void {
  const a = new Uint16Array(sizes.length * 2);
  const b = new Uint16Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Uint16Array", own, inclusive, offset, cross, bad);
}
checkUint16Array();

function checkInt32Array(): void {
  const a = new Int32Array(sizes.length * 2);
  const b = new Int32Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Int32Array", own, inclusive, offset, cross, bad);
}
checkInt32Array();

function checkUint32Array(): void {
  const a = new Uint32Array(sizes.length * 2);
  const b = new Uint32Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Uint32Array", own, inclusive, offset, cross, bad);
}
checkUint32Array();

function checkFloat16Array(): void {
  const a = new Float16Array(sizes.length * 2);
  const b = new Float16Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Float16Array", own, inclusive, offset, cross, bad);
}
checkFloat16Array();

function checkFloat32Array(): void {
  const a = new Float32Array(sizes.length * 2);
  const b = new Float32Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Float32Array", own, inclusive, offset, cross, bad);
}
checkFloat32Array();

function checkFloat64Array(): void {
  const a = new Float64Array(sizes.length * 2);
  const b = new Float64Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let own = 0, inclusive = 0, offset = 0, cross = 0, bad = 0;
  const k = 2;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i] * 2;
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i] * 2;
  for (let i = 0; i < a.length - k; i++) offset += a[i] + a[i + k];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i] * 2;
  for (let i = 0; i < a.length; i++) {
    const x = b[i], y = b[i];
    bad += x === undefined ? 100 : x + y;
  }
  console.log("Float64Array", own, inclusive, offset, cross, bad);
}
checkFloat64Array();

function checkBigInt64Array(): void {
  const a = new BigInt64Array(sizes.length * 2);
  const b = new BigInt64Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = BigInt(i + 1);
  let own = 0n, inclusive = 0n, offset = 0n, cross = 0n;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i];
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i];
  for (let i = 0; i < a.length - 2; i++) offset += a[i] + a[i + 2];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i];
  console.log("BigInt64Array", String(own), String(inclusive), String(offset), String(cross));
}
checkBigInt64Array();

function checkBigUint64Array(): void {
  const a = new BigUint64Array(sizes.length * 2);
  const b = new BigUint64Array(sizes.length);
  for (let i = 0; i < a.length; i++) a[i] = BigInt(i + 1);
  let own = 0n, inclusive = 0n, offset = 0n, cross = 0n;
  for (let i = 0; i < a.length; i++) own += a[i] + a[i];
  for (let i = 0; i <= a.length - 1; i++) inclusive += a[i] + a[i];
  for (let i = 0; i < a.length - 2; i++) offset += a[i] + a[i + 2];
  for (let i = 0; i < b.length; i++) cross += a[i] + a[i];
  console.log("BigUint64Array", String(own), String(inclusive), String(offset), String(cross));
}
checkBigUint64Array();

function detach(a: Float64Array): void { (a.buffer as any).transfer(); }
function detachOwn(at: number): void {
  const a = new Float64Array(sizes.length * 2);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let sum = 0, missing = 0, iterations = 0;
  for (let i = 0; i < a.length; i++) {
    sum += a[i] + a[i];
    if (i === at) detach(a);
    const x = a[i], y = a[i];
    if (x === undefined || y === undefined) missing++;
    iterations++;
  }
  console.log("detach own", at, sum, missing, iterations, a.length);
}
detachOwn(0); detachOwn(3); detachOwn(7);
function detachOther(): void {
  const a = new Float64Array(sizes.length * 2);
  const b = new Float64Array(sizes.length * 2);
  let missing = 0, count = 0;
  for (let i = 0; i < b.length; i++) {
    if (i === 2) detach(a);
    const x = a[i], y = a[i];
    if (x === undefined || y === undefined) missing++;
    count++;
  }
  console.log("detach accessed", count, missing, a.length, b.length);
}
detachOther();
function shrink(buffer: ArrayBuffer, bytes: number): void { (buffer as any).resize(bytes); }
function resizeLoop(fixed: boolean, grow: boolean): void {
  const buffer = new ArrayBuffer(64, { maxByteLength: 128 });
  const a = fixed ? new Float64Array(buffer, 0, 8) : new Float64Array(buffer);
  const b = new Float64Array(sizes.length * 2);
  for (let i = 0; i < a.length; i++) a[i] = i + 1;
  let sum = 0, missing = 0, count = 0;
  for (let i = 0; i < a.length; i++) {
    sum += a[i] + a[i];
    if (i === 2) shrink(buffer, grow ? 96 : 16);
    const x = a[i], y = a[i];
    if (x === undefined || y === undefined) missing++;
    count++;
  }
  let cross = 0;
  for (let i = 0; i < b.length; i++) {
    const x = a[i], y = a[i];
    cross += x === undefined ? 100 : x + y;
  }
  console.log("resize", fixed, grow, sum, missing, count, a.length, cross);
}
resizeLoop(false, false); resizeLoop(true, false); resizeLoop(false, true);
function edges(start: number, k: number): void {
  const a = new Float64Array(sizes.length);
  let sum = 0, missing = 0;
  for (let i = start; i < a.length - k; i++) {
    const x = a[i], y = a[i];
    if (x === undefined || y === undefined) missing++;
    else sum += x + y;
  }
  console.log("edges", start, k, sum, missing);
}
edges(-1, 0); edges(0.5, 0); edges(0, 6); edges(0, -2);
function reassign(): void {
  let a = new Float64Array(8);
  let count = 0;
  for (let i = 0; i < a.length; i++) {
    a[i] = a[i] + 1;
    if (i === 2) a = new Float64Array(2);
    count++;
  }
  console.log("reassign", count, a.length);
}
reassign();
function fake(a: Float64Array): number {
  let sum = 0;
  for (let i = 0; i < a.length; i++) sum += a[i] + a[i];
  return sum;
}
let reads = 0;
const fakeView = { 0: 3, 1: 4, get length() { reads++; return 2; } };
console.log("getter", fake(fakeView as any), reads);

function inclusiveOverrun(): void {
  const a = new Float64Array(sizes.length * 2);
  let missing = 0;
  for (let i = 0; i <= a.length - 1; i++) {
    const x = a[i + 1], y = a[i + 1];
    if (x === undefined || y === undefined) missing++;
  }
  console.log("inclusive overrun", missing);
}
inclusiveOverrun();
function negativeOffset(): void {
  const a = new Float64Array(sizes.length * 2);
  const k = -2;
  let missing = 0;
  for (let i = 0; i < a.length - k; i++) {
    const x = a[i], y = a[i];
    if (x === undefined || y === undefined) missing++;
  }
  console.log("negative offset", missing);
}
negativeOffset();
function rareCall(seed: number): number { return seed * 3; }
function rechecks(at: number): void {
  const a = new Float64Array(sizes.length * 2);
  let sum = 0;
  for (let i = 0; i < a.length; i++) {
    sum += a[i] + a[i] * 2;
    if (i === at) sum += rareCall(i);
  }
  console.log("recheck", sum);
}
rechecks(3);

function counterStart(start: number): number {
  const a = new Float64Array(sizes.length * 2);
  let s = 0;
  for (let i = start; i < a.length; i++) {
    const v = a[i];
    s += v === undefined ? 1000 : a[i] + a[i] * 2 + a[i] * 3 + a[i] * a[i];
  }
  return s;
}
console.log("counter entry", counterStart(-1), counterStart(0.5), counterStart(0));
