// Decision 150 for object-literal classes: a binding that holds literals of
// one `__AnonShape_*` class AND values from interpreted code (⊤) gets a
// CHECKED proof: the site compares the receiver's class id against the
// literal class and takes the generic read for anything else. The foreign
// object below has `x` at another slot and a different class id; a missing
// check would read its `y` as `x`.
// WPFACTS: elide .x checked
// WPFACTS: guard on
function pt(x: number, y: number) {
  return { x: x, y: y };
}
function gx(p: any): any {
  return p.x;
}
let t = 0;
for (let i = 0; i < 10; i++) t += gx(pt(i, 100 + i));
console.log(t);
const make = new Function("n", ["return { y: n, z: n * 2, x: 'foreign' + n };"].join(""));
console.log(gx(make(3)), gx(pt(7, 8)));
// Interpreted code rewrites a literal of another class (that class escapes;
// `pt`'s literals never reach interpreted code).
const poke = new Function("o", ["o.x = 'poked'; return o;"].join(""));
console.log(gx(poke({ w: 1, x: 2 })), gx(pt(1, 2)), gx(make(9)));
