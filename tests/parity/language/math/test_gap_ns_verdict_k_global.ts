// globalThis.Math replaced by an object literal.
const x = Number(process.argv[2] ?? "2");
console.log(Math.abs(-x));
(globalThis as any).Math = { abs: (_v: number) => "replaced" };
console.log(Math.abs(-x));
