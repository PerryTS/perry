function* numbers(start: number, end: number) {
  for (let i = start; i < end; i++) yield i;
}
function* mapped(it: Generator<number>, f: (x: number) => number) {
  for (const x of it) yield f(x);
}
let total = 0;
for (let repeat = 0; repeat < 3; repeat++) {
  for (const x of mapped(numbers(0, 3000), function (n: number) { return n % 17; })) {
    total += x;
    // Allocate through the ordinary producer while the parent, its callback
    // and its standalone input remain live across collections.
    numbers(0, 1);
  }
}
function* abrupt(f: () => void) {
  try { yield 10; yield 20; } finally { f(); yield 30; }
}
for (const x of abrupt(function () { total += 100; })) { total += x; break; }
try {
  for (const x of mapped(numbers(0, 4), function (n: number) {
    if (n === 2) throw 70;
    return n;
  })) total += x;
} catch (error) { total += error; }
console.log(total);
