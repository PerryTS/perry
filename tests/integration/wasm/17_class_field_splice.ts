// Class field array splice — tests that this.field.splice() works correctly
class LineBuffer {
  lines: number[];
  constructor() {
    this.lines = [10, 20, 30, 40, 50];
  }
  deleteRange(start: number, count: number): number[] {
    const deleted = this.lines.splice(start, count);
    return deleted;
  }
  insertAt(index: number, value: number): void {
    this.lines.splice(index, 0, value);
  }
  replaceAt(index: number, value: number): number[] {
    return this.lines.splice(index, 1, value);
  }
  getLength(): number {
    return this.lines.length;
  }
  getAt(index: number): number {
    return this.lines[index];
  }
}

const buf = new LineBuffer();
console.log(buf.getLength());

const del = buf.deleteRange(1, 2);
console.log(del.length);
console.log(del[0]);
console.log(del[1]);
console.log(buf.getLength());
console.log(buf.getAt(0));
console.log(buf.getAt(1));
console.log(buf.getAt(2));

buf.insertAt(1, 25);
console.log(buf.getLength());
console.log(buf.getAt(1));
console.log(buf.getAt(2));

const replaced = buf.replaceAt(0, 99);
console.log(replaced[0]);
console.log(buf.getAt(0));
console.log(buf.getLength());
