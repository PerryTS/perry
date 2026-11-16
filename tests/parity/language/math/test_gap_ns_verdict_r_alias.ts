// A write through an alias replaces Math.sin (ECMA-262 21.3: Math.sin is writable).
const x = Number(process.argv[2] ?? "1");
const M: any = Math;
M.sin = (_v: number) => 42;
console.log(Math.sin(x));
