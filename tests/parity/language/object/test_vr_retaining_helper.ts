// DESIGN-p4l4 §4.4: a helper that keeps its argument would keep a copy.
// `keep` is self-recursive so that the inliner leaves the call in place: an
// inlined `keep` is no helper (the record then goes into `saved` directly).
// vr-expect: sq,pc REJECTED R5RetainingHelper(keep, retain)
const saved: unknown[] = [];
function keep(x: unknown, depth: number): void {
  if (depth > 0) {
    keep(x, depth - 1);
    return;
  }
  saved.push(x);
}
function mk(s: number, p: number) {
  return { sq: s, pc: p };
}
keep(mk(1, 2), 1);
keep(5, 0);
console.log(saved.length, saved[0], saved[1]);
