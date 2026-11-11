// S0 `restlaneraw` (I32 at rest, PR C): a proven store into an I32 field lane
// writes the value's double word instead of its I32 word. The proven load's
// witness must report the slot at the next read.
class Acc {
  v = 0;
  bump(k: number): void {
    this.v = (this.v + k) & 1023;
  }
  read(): number {
    return this.v ^ 5;
  }
}
const a = new Acc();
let s = 0;
for (let i = 0; i < 20; i++) {
  a.bump(i);
  s = (s + a.read()) | 0;
}
console.log(s);
