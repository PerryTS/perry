// S0 `restlaneraw` (I32 at rest, PR C): a proven lane store writes the
// value's double word instead of its I32 word. The witness at the next
// proven lane read must report the slot.
class Acc {
  v = 0;
}
const a = new Acc();
let s = 0;
for (let i = 0; i < 20; i++) {
  a.v = (a.v + i) & 1023;
  s = (s + (a.v ^ 5)) | 0;
}
console.log(s);
