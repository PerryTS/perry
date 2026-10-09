// Ordinary indexed loops keep the historical hot length guard. Its cold
// boundary still observes getters, wrong annotations and forwarded growth.
function sum(a: number[]) {
  let s = 0;
  for (let i = 0; i < a.length; i++) s += a[i];
  return s;
}
console.log(sum([1,2,3,4]));
console.log(sum({0: 7, 1: 8, length: 2} as any));
let reads = 0;
console.log(sum({0: 2, 1: 4, get length() { reads++; return 2; }} as any), reads);
console.log(sum(new Float64Array([3,5,7]) as any));
function growing(a: number[]) {
  let s = 0;
  for (let i = 0; i < a.length; i++) {
    s += a[i];
    if (i === 0) for (let n = 3; n < 40; n++) a.push(n);
  }
  return s;
}
console.log(growing([1,2]));
