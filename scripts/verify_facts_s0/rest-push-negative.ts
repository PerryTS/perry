// S0 `restpush` (I32 at rest, push-filled births): the fill walk counts a
// push under an `if` in the counted loop as unconditional, so the Array is
// claimed 64 long while one iteration skips its push. The read of the last
// slot then meets a hole below the floor: the at-rest load witness must
// report it ("an at-rest slot holds no I32 word").
// 64 iterations: a loop the compiler does not unroll.
class T {
  private tab: number[] = [];
  constructor() {
    for (let i = 0; i < 64; i++) {
      if (i !== 37) this.tab.push(i * 3);
    }
  }
  at(i: number): number {
    return this.tab[i & 63] ?? -1;
  }
}
const t = new T();
let s = 0;
for (let i = 0; i < 64; i++) s = (s + t.at(i)) | 0;
console.log(s);
