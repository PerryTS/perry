const key = Symbol("schema")
const proto: any = { _tag: "Some", [key]: true, step(x: number) { return x + 1 } }
const a: any = Object.create(proto); a.value = 7
const fields = ["value", "_tag", "absent"]
let sum = 0
const t = performance.now()
for (let i = 0; i < 1000000; i++) {
  sum += a[fields[i % 3]] === undefined ? 1 : 2
  if (a[key]) sum += a.step(i) & 1
}
console.log(sum)
