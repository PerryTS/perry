// S0 `restlaneraw` (I32 at rest, PR C): a lane store writes the value's double
// word instead of its I32 word (an object-literal birth's lane argument and a
// proven store). The witness at the next proven or site-proven read of the
// lane must report the slot.
interface Cell {
  v: number;
}
function make(k: number): Cell {
  return { v: (k * 7) & 1023 };
}
let s = 0;
for (let i = 0; i < 20; i++) {
  const c = make(i);
  s = (s + (c.v ^ 5)) | 0;
}
console.log(s);
