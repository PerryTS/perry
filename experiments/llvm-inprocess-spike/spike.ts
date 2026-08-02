// Phase 0 differential module: small but touches strings, arrays, closures,
// classes, Map iteration, and async — the value shapes Perry's IR leans on.
function fib(n: number): number {
  return n < 2 ? n : fib(n - 1) + fib(n - 2);
}

const arr = [3, 1, 4, 1, 5, 9, 2, 6];
arr.sort((a, b) => a - b);
console.log("fib(20) =", fib(20));
console.log("sorted:", arr.join(","));

const m = new Map<string, number>();
m.set("alpha", 1);
m.set("beta", 2);
for (const [k, v] of m) console.log(k, "->", v);

class Point {
  x: number;
  y: number;
  constructor(x: number, y: number) {
    this.x = x;
    this.y = y;
  }
  norm(): number {
    return Math.sqrt(this.x * this.x + this.y * this.y);
  }
}
console.log("norm:", new Point(3, 4).norm());

const greet = (name: string) => `hello, ${name}!`;
console.log(greet("perry"));

(async () => {
  const v = await Promise.resolve(42);
  console.log("async:", v);
})();
