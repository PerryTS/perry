// m5: an argument birth stored into a field of an iteration-local holder: dead at the iteration.
class Holder {
  v: any = null;
  set(x: any): void {
    this.v = x;
  }
}
function run(n: number): number {
  let s = 0;
  for (let i = 0; i < n; i++) {
    const h = new Holder();
    h.set({ a: i, b: "w" });
    s += JSON.stringify(h.v).length;
  }
  return s;
}
console.log(run(200));
