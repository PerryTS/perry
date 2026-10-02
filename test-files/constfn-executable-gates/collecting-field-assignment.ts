declare function gc(): void;
let stored: any = null;
let junk: any = null;
let sets = 0;
function save(rhs: any): void {
  stored = rhs;
  sets++;
  for (let i = 0; i < 128; i++) junk = { x: i, text: "allocation-" + i };
  if (typeof gc === "function") gc();
}
class Cell {
  m: any = () => -1;
  write(value: number): number {
    const returned = this.m = () => value;
    return returned();
  }
}
const cell = new Cell();
Object.defineProperty(cell, "m", { get: () => stored, set: save, configurable: true });
let total = 0;
for (let i = 0; i < 12; i++) total += cell.write(i);
console.log(total, stored(), sets, junk.x);
