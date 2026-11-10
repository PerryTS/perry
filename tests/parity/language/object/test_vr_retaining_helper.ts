// DESIGN-p4l4 §4.4: a helper that keeps its argument would keep a copy.
// vr-expect: sq,pc REJECTED R5RetainingHelper(keep, retain)
const saved: unknown[] = [];
function keep(x: unknown) {
  saved.push(x);
}
function mk(s: number, p: number) {
  return { sq: s, pc: p };
}
keep(mk(1, 2));
keep(5);
console.log(saved.length, saved[0], saved[1]);
