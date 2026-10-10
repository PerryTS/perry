const key = Symbol("schema")
const a: any = { value: 7, [key]: true }
let sum = 0
for (let i = 0; i < 1000000; i++) { if (a[key]) sum += 1 }
console.log(sum)
