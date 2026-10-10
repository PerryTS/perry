const a: any = { value: 7 };
const fields = ["absent", "absent", "absent"];
let sum = 0;
for (let i = 0; i < 1000000; i++) {
  sum += a[fields[i % 3]] === undefined ? 1 : 2;
}
console.log(sum);
