// m4: the callee returns a literal, the caller drops it: alive at return, dead at the caller's return.
function make(i: number): any {
  if (i < 0) return make(i + 1);
  return { a: i, b: "k" };
}
function drive(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    s += JSON.stringify(make(i)).length;
  }
  return s;
}
console.log(drive(200));
