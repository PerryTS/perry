interface Point {
  x: number;
  y: number;
}

function lengthSquared(p: Point): number {
  return p.x * p.x + p.y * p.y;
}

const p: Point = { x: 3.5, y: 4.25 };
const before = JSON.stringify(p);
const dynamicX = (p as any)["x"];
const arithmetic = lengthSquared(p) + dynamicX;
(p as any).x = { label: "heap" };
const after = JSON.stringify(p);

console.log(before);
console.log(arithmetic);
console.log(after);
console.log((p as any).x.label);
