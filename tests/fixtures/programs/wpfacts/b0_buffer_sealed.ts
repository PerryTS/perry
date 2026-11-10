// B (positive): a typed-array field nobody exposes keeps ta_stable.
// WPFACTS: fact F6 ta_stable Arena2.data = 1
// WPFACTS: fact F1 readonly Arena2.data = 1
class Arena2 {
  data: Int32Array;
  size: number;
  constructor(n: number) {
    this.data = new Int32Array(n);
    this.size = 0;
  }
  push(v: number) {
    this.data[this.size] = v;
    this.size++;
  }
}
const a = new Arena2(8);
for (let i = 0; i < 5; i++) a.push(i * 3);
console.log(a.size, a.data[4]);
