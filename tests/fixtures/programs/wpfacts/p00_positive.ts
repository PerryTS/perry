// Positive control: nothing escapes, so the facts are ON.
// WPFACTS: fact F3 prefix_stable Point = 1
// WPFACTS: fact F3 fixed_layout Point = 1
// WPFACTS: fact F1 readonly Point.x = 1
// WPFACTS: fact F1' definite Point.y = 1
// WPFACTS: kind Point.x is num
// WPFACTS: site .x on
// WPFACTS: call .norm on
// WPFACTS: guard off
class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  norm(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }
}
function sum(ps: Point[]): number {
  let s = 0;
  for (let i = 0; i < ps.length; i++) s += ps[i].x + ps[i].norm();
  return s;
}
const ps: Point[] = [];
for (let i = 0; i < 10; i++) ps.push(new Point(i, i + 1));
console.log(sum(ps).toFixed(3));
