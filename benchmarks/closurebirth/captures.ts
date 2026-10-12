// Usage: captures.ts <iterations> <captures: 1|2|3> <birth|call>.
// Retaining the previous closure makes allocation and identity observable.
const n = Number(process.argv[2] || "200000");
const arity = Number(process.argv[3] || "1");
const mode = process.argv[4] || "birth";
function one(a: number) { return () => a; }
function two(a: number, b: number) { return () => a + b; }
function three(a: number, b: number, c: number) { return () => a + b + c; }
function run(count: number) {
  let previous = one(0);
  let sum = 0;
  if (mode === "call") {
    previous = three(1, 2, 3);
    for (let i = 0; i < count; i++) sum += previous();
  } else if (arity === 1) {
    for (let i = 0; i < count; i++) {
      const next = one(i % 97);
      if (previous === next) throw new Error("closure identity");
      sum += previous();
      previous = next;
    }
  } else if (arity === 2) {
    for (let i = 0; i < count; i++) {
      const next = two(i % 97, i % 11);
      if (previous === next) throw new Error("closure identity");
      sum += previous();
      previous = next;
    }
  } else {
    for (let i = 0; i < count; i++) {
      const next = three(i % 97, i % 11, i % 7);
      if (previous === next) throw new Error("closure identity");
      sum += previous();
      previous = next;
    }
  }
  return sum + previous();
}
for (let i = 0; i < 10; i++) run(20000);
console.log(run(n));
