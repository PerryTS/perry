const key = Symbol("schema")
const proto: any = { _tag: "Some", [key]: true, step(x: number) { return x + 1 } }
const a: any = Object.create(proto); a.value = 7

const fields = ["absent","absent","absent"]
let sum = 0
for (let i = 0; i < 1000000; i++) {
  sum += a[fields[i % 3]] === undefined ? 1 : 2
}
console.log(sum)
