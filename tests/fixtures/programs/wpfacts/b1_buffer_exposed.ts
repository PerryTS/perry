// B1/B2: a typed-array field whose buffer is observed and transferred.
// WPFACTS: fact F6 ta_stable Arena.left = 0
class Arena {
  left: Int32Array;
  constructor(n: number) {
    this.left = new Int32Array(n);
  }
}
const a = new Arena(4);
a.left[1] = 5;
const buf = a.left.buffer;
console.log(buf.byteLength, a.left[1]);
