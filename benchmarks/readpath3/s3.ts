const key = Symbol("schema")
const proto: any = { _tag: "Some", [key]: true }
const a: any = Object.create(proto); a.value = 7
let sum = 0
for (let i = 0; i < 1000000; i++) { if (a[key]) sum += 1 }
console.log(sum)
