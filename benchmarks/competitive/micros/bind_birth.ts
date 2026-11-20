// Constructor bind birth plus invocation; two-N subtraction removes warmup.
const N = +(process.argv[2] || 200000);
class Bound {
  value: number;
  method: (n: number) => number;
  constructor(value: number) { this.value = value; this.method = this.parse.bind(this); }
  parse(n: number) { return this.value + n; }
}
function kernel(n: number) {
  // Keep the accumulator double during warmup and measurement.
  let checksum = 0.5;
  for (let i = 0; i < n; i++) { const c = new Bound(i); checksum += c.method(1); }
  return checksum - 0.5;
}
for (let i = 0; i < 10; i++) kernel(20000);
console.log('measure:begin');
const checksum = kernel(N);
console.log('measure:end');
console.log(checksum);
