// S8: reads of declared fields and late keys of family objects through
// untyped read sites (the layout probe), and their absence.
class Point { x = 1; y = 2; label = "p"; }
class Point3 extends Point { z = 3; }
class Bag {}
function read(o: any): string {
  return `${o.x}/${o.y}/${o.label}/${o.z}/${o.missing}/${"x" in o}/${"z" in o}`;
}
function fillBag(i: number): any {
  const b: any = new Bag();
  b.x = i;
  if (i % 3 == 0) b.y = i * 2;
  b.label = "bag" + i;
  return b;
}
const out: string[] = [];
let sum = 0;
for (let i = 0; i < 2000; i++) {
  const p: any = i % 2 ? new Point() : new Point3();
  p.x = i;
  const b = fillBag(i);
  sum += p.x + p.y + (p.z ?? 0) + b.x + (b.y ?? 0);
  if (i < 6) out.push(read(p) + " " + read(b));
}
out.push("sum " + sum);
console.log(out.join("\n"));
