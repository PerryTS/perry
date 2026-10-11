const N = Number(process.argv[2] ?? 2000000);
const table: any = { string: 7, number: 11 };
const inputs: any[] = ['value', 42];
function read(o: any, value: any) { return o[typeof value]; }
let sum = 0;
for (let i = 0; i < N; i++) sum += read(table, inputs[i % 2]);
console.log(sum);
