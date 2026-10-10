const proto: any = { _tag: "Some", step(x: number) { return x + 1 } }
const a: any = Object.create(proto); a.value = 7
let sum = 0
for (let i = 0; i < 1000000; i++) { sum += a.step(i) & 1 }
console.log(sum)
