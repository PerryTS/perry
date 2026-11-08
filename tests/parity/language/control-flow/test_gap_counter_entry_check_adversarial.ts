// Loops whose counter bounds do not fit the view: every access outside the
// view is a no-op (store) or undefined (read), nothing is written beside it.
// Run via the compiled path with a canary on both sides of the view.

function canaryView(len: number): [Uint8Array, Uint8Array] {
  const backing = new Uint8Array(len + 16);
  backing.fill(0xaa);
  return [backing, backing.subarray(8, 8 + len)];
}

function sumOf(a: Uint8Array): number {
  let s = 0;
  for (let k = 0; k < a.length; k++) s = (s + a[k] * (k + 1)) | 0;
  return s;
}

// bound larger than the view, `<` and `<=`
function fillLt(view: Uint8Array, n: number): number {
  let c = 0;
  for (let i = 0; i < n; i++) {
    view[i] = 7;
    c++;
  }
  return c;
}
function fillLe(view: Uint8Array, n: number): number {
  let c = 0;
  for (let i = 0; i <= n; i++) {
    view[i] = 9;
    c++;
  }
  return c;
}
// read of the element at the counter, bound past the view
function readLt(view: Uint8Array, n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const v = view[i];
    s += v === undefined ? 1000 : v;
  }
  return s;
}
// start below zero
function fromNegative(view: Uint8Array, n: number): number {
  let c = 0;
  for (let i = -3; i < n; i++) {
    view[i] = 5;
    c++;
  }
  return c;
}
// index from a local whose value wraps in i32
function wrapped(view: Uint8Array, k: number): number {
  const idx = (k * 1073741825) | 0;
  let c = 0;
  for (let r = 0; r < 3; r++) {
    view[idx] = 3;
    c++;
  }
  return c + (idx < 0 ? 1 : 0);
}
// counter that starts near the top of i32
function nearTop(view: Uint8Array, base: number): number {
  let c = 0;
  for (let i = base; i < base + 4; i++) {
    view[i] = 4;
    c++;
  }
  return c;
}
// stride past the view
function strided(view: Uint8Array, n: number, s: number): number {
  let c = 0;
  for (let i = 0; i < n; i += s) {
    view[i] = 6;
    c++;
  }
  return c;
}

const out: number[] = [];
for (const n of [0, 5, 16, 17, 40, 100000]) {
  let [b, v] = canaryView(16);
  out.push(fillLt(v, n), sumOf(b));
  [b, v] = canaryView(16);
  out.push(fillLe(v, n), sumOf(b));
  [b, v] = canaryView(16);
  v.fill(2);
  out.push(readLt(v, n));
  [b, v] = canaryView(16);
  out.push(fromNegative(v, n), sumOf(b));
  [b, v] = canaryView(16);
  out.push(strided(v, n, 3), sumOf(b));
}
for (const k of [0, 1, 2, 3, 7, 1000, 2147483647]) {
  const [b, v] = canaryView(8);
  out.push(wrapped(v, k), sumOf(b));
}
for (const base of [0, 10, 14, 2147483640, 2147483644, 2147483647]) {
  const [b, v] = canaryView(16);
  out.push(nearTop(v, base), sumOf(b));
}
console.log(out.join(","));
