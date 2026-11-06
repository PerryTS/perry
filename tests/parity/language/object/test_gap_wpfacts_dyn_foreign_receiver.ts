// Decision 150: interpreted code is a summary node of the whole-program
// facts, not a program-wide kill. A function made by `new Function` (source
// built at run time, so it is interpreted) returns a foreign object that
// reaches a compiled function whose receiver the facts know holds `Pt`
// instances. The receiver's set is {Pt, ⊤}: its field reads are CHECKED
// (class id compare), so the foreign object, whose `x` lives at another
// slot, takes the generic read.
class Pt {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
}

function sx(p: any): any {
  return p.x;
}

function sumX(ps: any[]): number {
  let t = 0;
  for (let i = 0; i < ps.length; i++) t += sx(ps[i]);
  return t;
}

const pts = [new Pt(1, 2), new Pt(3, 4), new Pt(5, 6)];
console.log(sumX(pts));
const make = new Function("n", ["return { y: n, z: n * 2, x: 'foreign' + n };"].join(""));
const foreign = make(5);
console.log(sx(foreign), sx(pts[1]));
// The interpreted code also receives an instance of another class and
// rewrites it (that class escapes; `Pt` never reaches interpreted code).
class Q {
  w: number;
  x: number;
  constructor() {
    this.w = 0;
    this.x = 1;
  }
}
const poke = new Function("p", ["p.x = 'poked'; return p;"].join(""));
const given = poke(new Q());
console.log(sx(given), sumX(pts), sx(make(9)));
