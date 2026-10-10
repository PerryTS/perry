const proto: any = { _tag: "Some" }
const a: any = Object.create(proto); a.value = 7
let sum = 0
for (let i = 0; i < 1000000; i++) { sum += a._tag === undefined ? 1 : 2 }
console.log(sum)
