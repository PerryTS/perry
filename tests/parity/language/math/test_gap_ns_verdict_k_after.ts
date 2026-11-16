// The write comes AFTER the first call in program order.
const x = Number(process.argv[2] ?? "0");
console.log(Math.cos(x));
const M: any = Math;
M.cos = (_v: number) => -5;
console.log(Math.cos(x));
