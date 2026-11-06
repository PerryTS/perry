// D3 negative: stores that change the kind of a proven slot (string, undefined,
// null, boolean, NaN, Infinity), a nullish receiver, and evaluation order.
class Shape {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  describe(): number { return this.x + this.y; }
}
class Rect extends Shape {
  w: number;
  constructor(x: number, y: number, w: number) { super(x, y); this.w = w; }
}
const shapes: Shape[] = [new Shape(1, 2), new Rect(3, 4, 5)];
const vals: any[] = [7, "s", undefined, null, true, NaN, Infinity, -0, 1.5];
const out: string[] = [];
for (const v of vals) {
  for (let i = 0; i < shapes.length; i++) {
    const s: Shape = shapes[i];
    s.x = v;
    out.push(String(s.describe()) + ":" + Object.is(s.x, v));
  }
}
console.log(out.join(" "));
let count = 0;
let thrown = "none";
try {
  const t: Shape = shapes[5];
  t.x = count++;
} catch (e) { thrown = (e instanceof TypeError) + "/" + count; }
console.log(thrown);
let thrown2 = "none";
try {
  for (let i = 0; i <= shapes.length; i++) {
    const s: Shape = shapes[i];
    s.x = (s.x as number) + 1;
  }
} catch (e) { thrown2 = (e instanceof TypeError) + "/" + shapes[0].x + "/" + shapes[1].x; }
console.log(thrown2);
