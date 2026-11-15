// `t = t + a[i]` over a local bound to an Array literal of numbers: the Add proof took the literal as numeric storage
// whatever the body stored into it later, so the accumulator counted as a Number by construction and its numeric arm
// `fadd`ed the string it held after a string element (`1two` where node prints `1two3`). Every use other than an
// element or `length` read now voids the proof. Controls: unwritten literals (numeric sums), typed arrays, a
// numeric store, sums through a helper.
function direct(): any {
  const a = [1, 2, 3];
  (a as any)[1] = "two";
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function pushed(): any {
  const a = [1, 2, 3];
  (a as any[]).push("four", 5);
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function aliased(): any {
  const a = [4, 5, 6];
  const b: any = a;
  b[0] = "x";
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function writeFirst(arr: any): void {
  arr[0] = "w";
}
function viaCall(): any {
  const a = [7, 8, 9];
  writeFirst(a);
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function viaClosure(): any {
  const a = [1, 1, 1];
  const set = (k: number) => {
    (a as any)[k] = "c";
  };
  set(2);
  let t: any = 10;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function filled(): any {
  const a = [1, 2, 3];
  (a as any).fill("f", 1, 2);
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function twice(): string {
  // The same loop over the same literal before and after a string store.
  const a = [1, 2, 3];
  let out = "";
  for (let round = 0; round < 2; round++) {
    let t: any = 0;
    for (let i = 0; i < a.length; i++) t = t + a[i];
    out += String(t) + ";";
    (a as any)[0] = "s";
  }
  return out;
}
// Controls.
function untouched(): any {
  const a = [1.5, 2.5, 3];
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function numericStore(): any {
  const a = [1, 2, 3];
  a[1] = 20;
  let t = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function typed(): any {
  const a = new Float64Array([1, 2, 3]);
  (a as any)[1] = "5";
  let t: any = 0;
  for (let i = 0; i < a.length; i++) t = t + a[i];
  return t;
}
function outOfBounds(): any {
  const a = [1, 2];
  let t: any = 0;
  for (let i = 0; i <= a.length; i++) t = t + a[i];
  return t;
}
console.log(direct(), pushed(), aliased(), viaCall(), viaClosure(), filled());
console.log(twice());
console.log(untouched(), numericStore(), typed(), outOfBounds());
