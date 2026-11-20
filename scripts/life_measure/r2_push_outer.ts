// LIFE I3 fixture: every iteration of fill's loop stores its object into an
// older array (pre-filled with objects: no growth, no element-kind change):
// every iteration is dirty with exactly one survivor, held by that array.
const keep: { v: number }[] = [];
for (let i = 0; i < 200; i++) keep.push({ v: -1 });
function fill(n: number): number {
  for (let i = 0; i < n; i++) {
    keep[i] = { v: i };
  }
  return keep.length;
}
console.log(fill(200));
let s = 0;
for (let i = 0; i < keep.length; i++) s += keep[i].v;
console.log(s);
