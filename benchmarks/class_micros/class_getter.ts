class Counter { value: number; constructor() { this.value = 0; } get scaled(): number { return this.value * 3 + 7; } set input(x: number) { this.value = x & 255; } }
function kernel(n: number): number {
const c = new Counter(); let s = 0; for (let i = 0; i < n; i++) { c.input = i; s += c.scaled; } return s;
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
