// A local binding named Math is not the namespace.
const x = Number(process.argv[2] ?? "3");
function f(): number { const Math = { sin: (v: number) => v * 2 }; return Math.sin(x); }
console.log(f(), Math.sin(0));
