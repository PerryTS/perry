// Value records (DESIGN-p4l4 §4.3, L2): logging a record never makes it a
// heap object. The record is admitted; every form prints as Node prints it.
// vr-expect: move,score ADMITTED
function mk(m: number, s: number) {
  return { move: m, score: s };
}
const undo = mk(12, -3);
const ply = 4;
console.log(undo);
console.log("%o %j %s", undo, undo, undo);
console.log({ undo });
console.log("pv", { ply, undo });
console.log([undo, ply]);
console.log(`${undo.move}:${undo.score}`);
console.log(JSON.stringify(undo));
