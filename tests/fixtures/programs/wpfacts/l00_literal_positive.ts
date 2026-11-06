// Object-literal classes (`litclass`): a literal with no methods compiles to
// an `__AnonShape_*` class and is born with that class id in its header, so
// a binding the facts prove holds only such literals reads its birth fields
// with no receiver guard, like an instance of a user class.
// WPFACTS: elide .x unchecked
// WPFACTS: elide .y unchecked
// WPFACTS: guard off
function mk(i: number) {
  return { x: i, y: i * 2, tag: "p" + i };
}
function mk3(i: number) {
  return { x: i + 0.5, y: -i, z: i };
}
function weight(p: { x: number; y: number }): number {
  return p.x * 3 + p.y;
}
let s = 0;
for (let i = 0; i < 20; i++) s += weight(mk(i)) + weight(mk3(i));
console.log(s);
const a = mk(4);
const b = mk3(4);
console.log(a.tag, weight(a), weight(b), JSON.stringify(a), Object.keys(b).join(","));
