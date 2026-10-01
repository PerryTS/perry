// #10520: a captured, never-reassigned closure value must be captured by
// value. A truly self-capturing initializer still needs a live cell.
function make(i: number) {
  const f = (x: number) => x + i;
  return () => f(1);
}
const a = make(2);
const b = make(9);
console.log(a(), b());

function makeRecursive() {
  const f = (n: number): number => n <= 1 ? 1 : n * f(n - 1);
  return () => f(5);
}
console.log(makeRecursive()());
