// `function F(){}` + expandos, then `export default F;` (#10434).
function Point(this: any, x?: unknown, y?: unknown) {
  this.x = x;
  this.y = y;
}
Point.prototype.describe = function (this: any) {
  return "Point(" + String(this.x) + "," + String(this.y) + ")";
};
(Point as any).origin = "static-origin";
(Point as any).create = function (x: unknown) {
  return new (Point as any)(x, "via-static");
};
export const holder = { Point };
export function isPoint(value: unknown) {
  return value instanceof Point;
}
export function makeLocal() {
  return new (Point as any)("local", 1);
}
export default Point;
