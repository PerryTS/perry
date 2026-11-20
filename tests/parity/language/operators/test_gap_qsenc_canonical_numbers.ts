function classify(v: any): number {
  let number: any = 1.25;
  let result = 0;
  if (v >= 100 && number < 200) result++;
  if (v < 100 || number >= 200) result += 2;
  return result;
}
const count = Number(process.argv[2] || 200000);
const values: any[] = [0, -1, 100, 127, 128, 60000, NaN, Infinity, -Infinity];
let sum = 0;
for (let i = 0; i < count; i++) sum += classify(values[i % values.length]);
console.log(sum);
let number: any = 1.25;
number = '250';
console.log(100 >= 100 && number < 200);
let calls = 0;
const obj = { valueOf() { calls++; return 100; } };
console.log(obj >= 100 && 1.25 < 200, calls);
