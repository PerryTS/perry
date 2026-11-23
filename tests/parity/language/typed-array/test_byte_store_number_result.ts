// A Number-returning string method on a proven string stored into a
// Uint8Array: the element takes the value's ToUint8, and the assignment
// evaluates to the Number itself, so an out-of-range `charCodeAt` stores 0
// but evaluates to NaN. `atob`'s result is a proven string too (#14970).
const lit = "Aÿz";
const v = new Uint8Array(3);
console.log("top", (v[2] = lit.charCodeAt(9)), (v[0] = lit.indexOf("z")), v[2], v[0]);
{
  const t = "Aÿz";
  const w = new Uint8Array(3);
  console.log("block", (w[2] = t.charCodeAt(9)), (w[1] = t.charCodeAt(1)), w[2], w[1], w[0]);
}
function storeCharCodes(): void {
  const t = "Aÿz";
  const w = new Uint8Array(3);
  console.log("fn", (w[2] = t.charCodeAt(9)), (w[0] = t.lastIndexOf("A")), w[2], w[1], w[0]);
}
storeCharCodes();
function decode(b64: string): void {
  const s = atob(b64);
  let u = new Uint8Array(0);
  for (let r = 0; r < 2; r++) {
    u = new Uint8Array(s.length + 1);
    for (let i = 0; i <= s.length; i++) u[i] = s.charCodeAt(i);
  }
  console.log("atob", u.join(","), (u[0] = s.charCodeAt(99)), u[0], (u[1] = s.charCodeAt(0)));
}
decode("/wBBYg==");
