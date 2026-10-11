class Row { x: number; y: number; constructor(x: number) { this.x = x; this.y = 3; } }
function kernel(n: number): number {
const a: Row[] = []; for (let j = 0; j < 256; j++) a.push(new Row(j)); let s = 0; for (let i = 0; i < n; i++) { const r = a[i & 255]; s += r.x * r.y + 7; } return s;
}
const n = Number(process.argv[2] || '200000');
let warm = 0; for (let w = 0; w < 10; w++) warm += kernel(20000);
console.log('measure:begin');
const start = Date.now();
const result = kernel(n);
const elapsed = Date.now() - start;
console.log('measure:end');
console.log('checksum:' + result + ':' + warm);
console.log('elapsed_ms:' + elapsed);
