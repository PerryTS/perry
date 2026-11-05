// #11973: an array truncated through an element-store node (a for-of/for-in
// head target, a destructuring target, a computed "length" key) must not keep
// its stale literal length in the specialized typed-array clone, or reads past
// the real end return neighbouring memory instead of NaN/undefined.
// One reader per variant: the specialized clone is chosen per call site.
function s1(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s2(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s3(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s4(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s5(t: Float64Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s6(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s7(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s8(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s9(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s10(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}
function s11(t: Int32Array) {
  let c = 0;
  for (let i = 0; i < 4; i++) c += t[i];
  return c;
}

const a1 = [1, 2, 3, 4];
for (a1["length"] of [2]) {}
const t1 = new Int32Array(a1);

const a2 = [1, 2, 3, 4];
const k2 = "length";
for (a2[k2] of [2]) {}
const t2 = new Int32Array(a2);

const a3 = [1, 2, 3, 4];
for (a3["length"] in { "2": 1 }) {}
const t3 = new Int32Array(a3);

const a4 = [1, 2, 3, 4];
[a4["length"]] = [2];
const t4 = new Int32Array(a4);

const a5 = [1.5, 2.5, 3.5, 4.5];
for (a5["length"] of [2]) {}
const t5 = new Float64Array(a5);

const a6 = [1, 2, 3, 4];
a6["length"] = 2;
const t6 = new Int32Array(a6);

const a7 = [1, 2, 3, 4];
Object.defineProperty(a7, "length", { value: 2 });
const t7 = new Int32Array(a7);

const a8 = [1, 2, 3, 4];
Reflect.set(a8, "length", 2);
const t8 = new Int32Array(a8);

const a9 = [1, 2, 3, 4];
const alias9 = a9;
alias9.splice(2, 2);
const t9 = new Int32Array(a9);

const a10 = [1, 2, 3, 4];
const alias10 = a10;
alias10.pop();
alias10.pop();
const t10 = new Int32Array(a10);

// Growing and element stores through an index stay exact.
const a11 = [1, 2, 3, 4];
for (let i = 0; i < 4; i++) a11[i] = i + 10;
const t11 = new Int32Array(a11);

// Neighbours allocated after the truncated copies: reads past the end of a
// stale-length copy would return their contents.
const n1 = new Int32Array([7, 7, 7, 7, 7, 7, 7, 7]);
const n2 = new Float64Array([9, 9, 9, 9, 9, 9, 9, 9]);

console.log(t1.length, s1(t1));
console.log(t2.length, s2(t2));
console.log(t3.length, s3(t3));
console.log(t4.length, s4(t4));
console.log(t5.length, s5(t5));
console.log(t6.length, s6(t6));
console.log(t7.length, s7(t7));
console.log(t8.length, s8(t8));
console.log(t9.length, s9(t9));
console.log(t10.length, s10(t10));
console.log(t11.length, s11(t11));
console.log(n1[0], n2[0]);
