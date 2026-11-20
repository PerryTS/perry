// `new TA(x)` owns fresh inline storage only when `x` is a LENGTH, i.e. never
// an Object. A Number argument (a literal, an integer local, a specialized
// entry's numeric parameter) is a length; an ArrayBuffer is the view form, a
// typed array and an array-like the copy forms. The compiler must never treat
// those as owned-by-length: a write would land in a private copy instead of
// the shared buffer, and a read would see stale or wrong elements.

function make(n: any): Int32Array {
  const a = new Int32Array(n);
  a[0] = 7;
  let k = a[0] - 6;
  a[k] = a[k - 1] + 1;
  return a;
}

function sized(len: number): number {
  let m = 0;
  for (let i = 0; i < len; i++) m++;
  const a = new Int32Array(m);
  for (let i = 0; i < m; i++) a[i] = i * 3;
  let k = a[1];
  let s = 0;
  while (k < m * 3) {
    s += a[k % m];
    k = k + a[1];
  }
  return s + a.length;
}

// A literal call site: the specialized entry may prove `n` a Number.
console.log("len:", Array.from(make(4)).join(","));

// The same function with an ArrayBuffer: the view form.
const buf = new ArrayBuffer(16);
const view = make(buf);
console.log("view:", Array.from(view).join(","), Array.from(new Int32Array(buf)).join(","));
view[3] = 42;
console.log("view shared:", new Int32Array(buf)[3]);

// A typed array and an array-like: copies, independent of their source.
const src = new Int32Array([1, 2, 3]);
const copy = make(src);
console.log("copy:", Array.from(copy).join(","), "src:", Array.from(src).join(","));
const like = make({ length: 3, 0: 5, 1: 6, 2: 9 });
console.log("array-like:", Array.from(like).join(","));
const arr = make([4, 5, 6, 7]);
console.log("array:", Array.from(arr).join(","));

// A length local that is later rebound to a buffer is not a length.
function rebound(): string {
  let n: any = 4;
  const b = new ArrayBuffer(8);
  if (n === 4) n = b;
  const a = new Int32Array(n);
  a[1] = 9;
  return Array.from(a).join(",") + " " + new Int32Array(b)[1];
}
console.log("rebound:", rebound());

console.log("sized:", sized(5), sized(9));

// A parameter proven a Number at entry but rebound in the body is not a length.
function reboundParam(n: number): string {
  const b = new ArrayBuffer(8);
  if (n > 0) n = b as any;
  const a = new Int32Array(n);
  a[1] = 9;
  return Array.from(a).join(",") + " " + new Int32Array(b)[1];
}
console.log("rebound param:", reboundParam(4));
