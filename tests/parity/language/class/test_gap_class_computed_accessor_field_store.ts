// THR1: a computed accessor whose key is known only at run time can take the
// name of a field the constructor assigns: `this.x = v` must call it.
function pick(s: string): any { return process.argv.length > 99 ? "never" : s; }
const seen: number[] = [];
class G {
  [Symbol.iterator]() { return [this.y][Symbol.iterator](); }
  constructor(x: number, y: number) { (this as any).x = x; (this as any).y = y; }
  set [pick("x")](v: number) { seen.push(v); }
  get [pick("x")]() { return -1; }
  sum() { return (this as any).x + (this as any).y; }
}
const g: any = new G(5, 6);
console.log(seen.join(), g.x, g.y, g.sum(), Object.keys(g).join(), [...g].join());
class H {
  x: number;
  constructor(x: number) { this.x = x; }
  get ["x2"]() { return this.x * 2; }
  *[Symbol.iterator]() { yield this.x; }
}
const h = new H(4);
h.x = 9;
console.log(h.x, (h as any).x2, [...h].join(), Object.keys(h).join());
