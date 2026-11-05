// #12531: dead functions' materialized default prototypes must be reclaimed.
// Run Node with --expose-gc for the memory assertion; the standard parity
// harness's Node arm checks the deterministic semantic checksum as well.
declare function gc(): void;
const canCollect = typeof gc === 'function';
function collect() { if (canCollect) { gc(); gc(); } }
function churn(n: number): number {
  let valid = 0;
  for (let i = 0; i < n; i++) {
    const f: any = function () { return i; };
    if (f.prototype.constructor === f) valid++;
  }
  return valid;
}
churn(1000);
collect();
const before = process.memoryUsage().heapUsed;
console.log('constructors', churn(100000));
collect();
const after = process.memoryUsage().heapUsed;
console.log('dead prototypes reclaimed', !canCollect || after - before < 4 * 1024 * 1024);
