// An i32 entry miss must leave the checked loop with Number storage.
class BoundaryRow {
  x: number;
  y: number;
  constructor(x: number) { this.x = x; this.y = 3; }
}
function boundary(n: number): number {
  const a: BoundaryRow[] = [];
  for (let j = 0; j < 8; j++) a.push(new BoundaryRow(j));
  let sum = 0;
  for (let i = 2147483646; i < n; i++) {
    const r = a[i & 7];
    sum += r.x * r.y + 7;
  }
  return sum;
}
console.log('integer-boundary', boundary(Number(process.argv[2] || '2147483649')));
