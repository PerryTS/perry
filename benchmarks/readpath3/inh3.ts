const proto: any = { value: 7 }
const a: any = Object.create(Object.create(Object.create(proto)))
const fields = ["value", "value", "value"]
let sum = 0
for (let i = 0; i < 1000000; i++) { sum += a[fields[i % 3]] === undefined ? 1 : 2 }
console.log(sum)
