// Spec-ABI soundness: a typed array created in VIEW form
// (`new Int32Array(buffer)`) must never take the guard-free specialized
// entry — its storage belongs to an ArrayBuffer that can be detached by
// `buffer.transfer()`. Outputs must stay byte-exact before and after detach.
function sum3(a: any) {
  let s = 0;
  s += a[0];
  s += a[1];
  s += a[2];
  return s;
}

const P = new Int32Array(3);
P[0] = 10;
P[1] = 20;
P[2] = 30;
console.log("inline:", sum3(P));

const buf = new ArrayBuffer(12);
const V = new Int32Array(buf);
V[0] = 1;
V[1] = 2;
V[2] = 3;
console.log("view:", sum3(V));

const moved = buf.transfer();
// Detached view: every element read is undefined, so the sum is NaN.
console.log("detached:", sum3(V));
console.log("byteLength:", buf.byteLength, moved.byteLength);
