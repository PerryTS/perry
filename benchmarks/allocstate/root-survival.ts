// Nursery construction followed by promotion and mutation of surviving roots.
class Cell {
  left: Cell | null;
  right: Cell | null;
  constructor(left: Cell | null, right: Cell | null) {
    this.left = left;
    this.right = right;
  }
}
function tree(n: number): Cell {
  return n === 0 ? new Cell(null, null) : new Cell(tree(n - 1), tree(n - 1));
}
function count(c: Cell | null): number {
  return c === null ? 0 : 1 + count(c.left) + count(c.right);
}
const roots: Cell[] = [];
let sum = 0;
for (let i = 0; i < 3000; i++) {
  roots[i % 32] = new Cell(tree(7), roots[(i + 1) % 32] || null);
  if (i % 32 === 31) {
    for (let j = 0; j < 32; j++) {
      roots[j].right = null;
      sum += count(roots[j]);
    }
  }
}
console.log(sum, roots.length, count(roots[0].left));
