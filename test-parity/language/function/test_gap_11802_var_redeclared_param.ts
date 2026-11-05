// #11802: a `var` re-declaration of a parameter rebinds it, so the function
// must not keep a specialized numeric entry that still treats it as the
// number the caller passed.
function viewOf(n: number): number {
  const b = new ArrayBuffer(8);
  var n: number = b as any; // rebinds the parameter to the ArrayBuffer
  const a = new Int32Array(n); // a view over b: length 2
  a[0] = 7;
  return a.length * 100 + new Int32Array(b)[0];
}
let s = 0;
for (let i = 0; i < 3; i++) s += viewOf(4);
console.log(s);

// The same rebinding through assignment, and through a conditional
// re-declaration, with the specialized call shape (a literal number).
function assigned(n: number): number {
  const b = new ArrayBuffer(16);
  n = b as any;
  const a = new Int32Array(n);
  return a.length;
}
function conditional(n: number, flip: boolean): number {
  const b = new ArrayBuffer(12);
  if (flip) {
    var n: number = b as any;
  }
  return new Int32Array(n).length;
}
console.log(assigned(4), conditional(4, true), conditional(4, false));

// An untouched numeric parameter still means a length.
function plain(n: number): number {
  return new Int32Array(n).length;
}
console.log(plain(5), plain(6));
