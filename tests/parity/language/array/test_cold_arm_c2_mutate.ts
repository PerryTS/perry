// b2 C2: in a covered loop, a getter on the slow arm changes the receiver's
// kind, length, storage or element descriptors; later reads see each change.
let a: number[] = [];
let mode = 0;
const S: any = "s";
const ACC: any = "acc";
Object.defineProperty(Array.prototype, "1.5", {
  configurable: true,
  get() {
    if (mode === 0) (a as any)[0] = "s";
    else if (mode === 1) a.length = 2;
    else if (mode === 2) for (let i = 0; i < 1000; i++) a.push(i);
    else Object.defineProperty(a, 3, { get() { return "acc"; }, configurable: true });
    return 0;
  },
});
function run(n: number): string {
  let s = 0, und = 0, acc = 0, num = 0;
  for (let k = 0; k < n; k++) {
    if (a[k & 3] === S) s++;
    if (a[k & 3] === undefined) und++;
    if (a[k & 3] === ACC) acc++;
    if (a[k & 3] === (k & 3) + 1) num++;
    if (k === 1 && a[k + 0.5] === 0) num += 100;
  }
  return s + " " + und + " " + acc + " " + num;
}
for (mode = 0; mode < 4; mode++) {
  a = [1, 2, 3, 4, 5, 6, 7, 8];
  console.log(mode, run(12), a.length);
}
delete (Array.prototype as any)["1.5"];
