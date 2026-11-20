// No write: every namespace call is the original builtin.
const x = Number(process.argv[2] ?? "0.5");
function f(v: number): number { return Math.sin(v) + Math.cos(v) * Math.sqrt(v + 1); }
let s = 0;
for (let i = 0; i < 100; i++) s += f(x + i / 100);
console.log(s.toFixed(9), Math.max(1, x), JSON.stringify({ a: Math.floor(x * 10) }));
