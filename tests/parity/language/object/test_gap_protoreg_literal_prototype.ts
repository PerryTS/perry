// Module-singleton source-ordered prototypes must be usable before any GC.
const token = Symbol("literal-prototype")
let order = ""
const holder: any = {
  tag: (order += "a", "Some"),
  [token]: (order += "b", true),
  step(x: number) { return x + 1 },
  get doubled() { return this.value * 2 },
  set doubled(v: number) { this.value = v / 2 }
}
const alias = holder
const receiver: any = Object.create(alias)
receiver.value = 7
const keys = ["value", "tag", "absent"]
let total = 0
for (let i = 0; i < 100000; i++) {
  total += receiver[keys[i % 3]] === undefined ? 1 : 2
  if (receiver[token]) total += receiver.step(i) & 1
}
console.log(order, total, receiver.doubled)
receiver.doubled = 22
console.log(receiver.value, Object.hasOwn(receiver, "value"), Object.hasOwn(holder, "value"))
holder.step = function(x: number) { return x + 3 }
holder[token] = false
holder.tag = "Changed"
console.log(receiver.step(5), receiver[token], receiver.tag)
delete holder.tag
console.log(receiver.tag === undefined)
const other: any = { [token]: 9, step(x: number) { return x * 2 } }
Object.setPrototypeOf(receiver, other)
console.log(receiver[token], receiver.step(5), Object.getPrototypeOf(receiver) === other)
