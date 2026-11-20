// Functions, recursion, closures
function add(a: number, b: number): number {
  return a + b;
}
console.log(add(3, 4));

function fib(n: number): number {
  if (n <= 1) return n;
  return fib(n - 1) + fib(n - 2);
}
console.log(fib(10));

function makeCounter(): () => number {
  let count = 0;
  return () => {
    count = count + 1;
    return count;
  };
}
const counter = makeCounter();
console.log(counter());
console.log(counter());
console.log(counter());
