"use strict";
// D3 negative: a frozen or sealed object, a non-writable field and a store
// through a subclass setter must not take a raw proven-slot store.
class Shape {
  x: number; y: number;
  constructor(x: number, y: number) { this.x = x; this.y = y; }
  describe(): number { return this.x + this.y; }
}
class Locked extends Shape {
  constructor(x: number, y: number) { super(x, y); Object.defineProperty(this, "y", { writable: false }); }
}
const a: Shape = new Shape(1, 2);
const b: Shape = new Locked(3, 4);
const c: Shape = new Shape(5, 6);
Object.freeze(c);
const out: string[] = [];
for (const s of [a, b, c]) {
  try { s.x = 10; } catch (e) { out.push("Ex"); }
  try { s.y = 20; } catch (e) { out.push("Ey"); }
  out.push(String(s.describe()));
}
console.log(out.join("|"));
