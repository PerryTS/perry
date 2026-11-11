// DESIGN-p4l4 §4.4: a logging helper called only with records has a Record
// parameter, carried by its one typed signature: the record crosses as its
// lanes (VR-3b) and `console.log` takes a copy. `show` is self-recursive so
// that the inliner leaves the calls in place.
// vr-expect: move,score ADMITTED
function mk(m: number, s: number) {
  return { move: m, score: s };
}
function show(x: { move: number; score: number }, depth: number): void {
  if (depth > 0) {
    show(x, depth - 1);
    return;
  }
  console.log(x);
}
const undo = mk(12, -3);
show(mk(1, 2), 1);
show(undo, 0);
