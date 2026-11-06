// Native compile test: class field array splice (direct, no workaround)
class LineStore {
  items: number[];
  constructor() {
    this.items = [10, 20, 30, 40, 50];
  }
  getLen(): number {
    return this.items.length;
  }
  getAt(i: number): number {
    return this.items[i];
  }
  deleteMid(): void {
    this.items.splice(1, 2);
  }
  insertAt(idx: number, val: number): void {
    this.items.splice(idx, 0, val);
  }
}
const b = new LineStore();
console.log(b.getLen());
b.deleteMid();
console.log(b.getLen());
console.log(b.getAt(0));
console.log(b.getAt(1));
console.log(b.getAt(2));
b.insertAt(1, 25);
console.log(b.getLen());
console.log(b.getAt(1));
