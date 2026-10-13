// Empty and one-field Object.create births isolate decode wrapper allocation.
const count = Number(process.argv[2] || "200000")
const mode = Number(process.argv[3] || "0")
const proto = Object.freeze({ kind: "wrapper" })
let sink: any = undefined
function kernel(n: number): number {
  let sum = 0
  for (let i = 0; i < n; i++) {
    const object: any = Object.create(proto)
    if (mode !== 0) object.value = i
    sink = object
    sum += mode !== 0 ? object.value : i
  }
  return sum
}
kernel(64)
console.log(kernel(count), Object.getPrototypeOf(sink) === proto)
