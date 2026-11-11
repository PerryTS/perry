// S0 `restlaneraw` (I32 at rest, PR C): a lane store writes the value's double
// word instead of its I32 word. The witness at the next lane read must report
// the slot.
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
