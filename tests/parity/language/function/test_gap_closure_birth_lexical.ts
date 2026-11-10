class Receiver {
  n: number;
  constructor(n: number) { this.n = n; }
  make(a: number) { return (b: number) => this.n + a + b; }
}
const left = new Receiver(10);
const right = new Receiver(100);
const f = left.make(3);
const g = right.make(4);
console.log(f(5), g(6), f.call(right, 5));
const cap = { text: "alive" };
function objectCapture(a: any, b: any, c: any) { return () => [a, b, c]; }
const keep = objectCapture(cap, [1, 2], "third");
for (let i = 0; i < 10000; i++) {
  const c = objectCapture({ text: "x" }, [i], "y");
  if (c()[1][0] !== i) throw new Error("capture");
}
const values = keep();
console.log(values[0] === cap, values[0].text, values[1].join(","), values[2]);
