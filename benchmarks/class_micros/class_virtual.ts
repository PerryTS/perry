class Base { scale: number; constructor(k: number) { this.scale = k; } step(x: number): number { return (x & 255) * this.scale + 7; } } class Derived extends Base { step(x: number): number { return (x & 255) * this.scale + 11; } }
function kernel(n: number): number {
const cs: Base[] = [new Base(3), new Derived(5)]; let s = 0; for (let i = 0; i < n; i++) s += cs[i & 1].step(i); return s;
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
