// A typed array whose tracked view lost its alias proof (it escaped to an
// unknown callee, a spread or Reflect call, or a dynamic call) keeps its
// element reads correct: in bounds, out of bounds, after the callee wrote
// the elements, after `.buffer` is exposed and after the buffer is detached.
// Compared byte-for-byte against `node --experimental-strip-types`.

let sink: any = null;
function keep(x: any) { sink = x; }
const dyn: any = keep;
function scribble(x: any) { x[3] = 100.5; x[15] = -1; }
const scribbleDyn: any = scribble;

function show(label: string, v: unknown) { console.log(label + ":", v); }

function sumRange(a: Float64Array, lo: number, hi: number): number {
  let s = 0;
  for (let i = lo; i < hi; i++) s += a[i];
  return s;
}

export function escapedThenRead(): void {
  const a = new Float64Array(16);
  for (let i = 0; i < 16; i++) a[i] = i * 0.5;
  dyn(a);
  let s = 0;
  for (let r = 0; r < 3; r++) for (let i = 0; i < 16; i++) s += a[i];
  show("escaped sum", s);
  // Out of bounds: undefined as a value, NaN in arithmetic.
  show("escaped oob", [a[16], a[-1 as number], a[1000]].join(","));
  let t = 0;
  for (let i = 14; i < 18; i++) t += a[i];
  show("escaped oob sum", t);
}

export function calleeWrites(): void {
  const a = new Float64Array(16);
  for (let i = 0; i < 16; i++) a[i] = i;
  scribbleDyn(a);
  let s = 0;
  for (let i = 0; i < 16; i++) s += a[i];
  show("callee wrote", [a[3], a[15], s].join(","));
}

export function spreadAndReflect(): void {
  const a = new Int32Array(8);
  for (let i = 0; i < 8; i++) a[i] = i * 3 - 4;
  const args: any[] = [a];
  keep(...args);
  Reflect.apply(keep, null, [a]);
  let s = 0;
  for (let i = 0; i < 8; i++) s += a[i];
  show("spread", [s, a[0], a[7], a[8]].join(","));
}

export function bufferExposedAfterEscape(): void {
  const a = new Float32Array(8);
  for (let i = 0; i < 8; i++) a[i] = i + 0.25;
  dyn(a);
  let before = 0;
  for (let i = 0; i < 8; i++) before += a[i];
  const view = new Float32Array(a.buffer);
  view[2] = 42;
  let after = 0;
  for (let i = 0; i < 8; i++) after += a[i];
  show("buffer", [before, after, a[2], a.buffer.byteLength].join(","));
}

export function detachedAfterEscape(): void {
  const a = new Float64Array(8);
  for (let i = 0; i < 8; i++) a[i] = i;
  dyn(a);
  let before = 0;
  for (let i = 0; i < 8; i++) before += a[i];
  const moved = (a.buffer as any).transfer();
  let after = 0;
  for (let i = 0; i < 8; i++) after += a[i];
  show("detached", [before, after, a.length, a[0], moved.byteLength].join(","));
}

export function capturedAndReassigned(): void {
  let a = new Float64Array(4);
  for (let i = 0; i < 4; i++) a[i] = i + 1;
  dyn(a);
  const swap = () => { a = new Float64Array([10, 20, 30, 40]); };
  let s = 0;
  for (let i = 0; i < 4; i++) { s += a[i]; if (i === 1) swap(); }
  show("reassigned", s);
}

escapedThenRead();
calleeWrites();
spreadAndReflect();
bufferExposedAfterEscape();
detachedAfterEscape();
capturedAndReassigned();
show("param", sumRange(new Float64Array([1, 2, 3, 4]), 1, 6));
