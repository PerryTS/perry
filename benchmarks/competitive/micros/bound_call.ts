// A reused bound method; two-N subtraction removes construction and warmup.
const N = +(process.argv[2] || 200000);
class BoundCall {
  value = 7;
  parse(n: number) { return this.value + n; }
}
const c = new BoundCall();
const bound = c.parse.bind(c);
function kernel(n: number) {
  // Keep the accumulator double during warmup and measurement.
  let checksum = 0.5;
  for (let i = 0; i < n; i++) checksum += bound(i);
  return checksum - 0.5;
}
for (let i = 0; i < 10; i++) kernel(20000);
console.log('measure:begin');
const checksum = kernel(N);
console.log('measure:end');
console.log(checksum);
