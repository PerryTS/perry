// A refused namespace call shares its offset with a member call on its result.
const M: any = Math;
M.sqrt = (_v: number) => 3;
const x = Number(process.argv[2] ?? "4");
console.log(Math.sqrt(x).toFixed(1), Math.floor(x + 0.5).toString());
