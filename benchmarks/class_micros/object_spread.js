function kernel(n        )         {
const base = { x: 3, y: 5, z: 7 }; let s = 0; for (let i = 0; i < n; i++) { const r = { ...base, x: i & 255 }; const { x, y, z } = r; s += x * y + z; } return s;
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
