// Exercise one read site across complete data, absent and getter answers.
function read(o: any): any { return o.answer }
const h1: any = { answer: 1 }, h2: any = { answer: 2 }
const a: any = Object.create(h1), b: any = Object.create(h2)
let sum = 0
for (let i = 0; i < 100; i++) sum += read(a) + read(b)
console.log("holders", sum)
h1.answer = 3
console.log("overwrite", read(a), read(b))
a.answer = 4
console.log("own", read(a), read(b))
delete a.answer
console.log("delete-own", read(a), read(b))
let calls = 0
Object.defineProperty(h1, "answer", { get() { calls++; return this.id }, configurable: true })
a.id = 7
console.log("getter", read(a), read(b), calls)
Object.defineProperty(h1, "answer", { value: null, writable: true, configurable: true })
console.log("null", read(a), read(b))
h1.answer = undefined
console.log("undefined", read(a), read(b))
h1.answer = 8
console.log("data-again", read(a), read(b))
h1.answer = null
console.log("null-after-prime", read(a), read(b))
h1.answer = undefined
console.log("undefined-after-prime", read(a), read(b))
delete h1.answer
console.log("delete-holder", read(a), read(b))
const g: any = { get answer() { calls++; return this.id } }
const x: any = Object.create(g), y: any = Object.create(g)
x.id = 11; y.pad = true; y.id = 13
sum = 0
for (let i = 0; i < 100; i++) sum += read(x) + read(y)
console.log("getter-receivers", sum, calls)
Object.defineProperty(g, "answer", { get() { return this.id + 10 }, configurable: true })
console.log("getter-replaced", read(x), read(y))
const middle: any = Object.create(h2), deep: any = Object.create(middle)
console.log("deep", read(deep), read(deep))
middle.answer = 19
console.log("intermediate-shadow", read(deep), read(deep))
delete middle.answer
console.log("intermediate-delete", read(deep))
Object.setPrototypeOf(middle, { answer: 23 })
console.log("intermediate-relink", read(deep))
Object.setPrototypeOf(deep, { answer: 29 })
console.log("receiver-relink", read(deep), read(b))
class Base { get answer() { return this.id } id = 31 }
class Child extends Base {}
const child: any = new Child()
console.log("mixed", read(child), read(child))
Object.setPrototypeOf(child, { answer: 37 })
console.log("class-relink", read(child))
function C(this: any) { this.id = 41 }
C.prototype.answer = 43
const old: any = new (C as any)()
console.log("prototype-before", read(old), read(old))
C.prototype = { answer: 47 }
const fresh: any = new (C as any)()
console.log("prototype-after", read(old), read(fresh), read(old), read(fresh))
let traps = 0
const proxy: any = new Proxy({ answer: 53 }, { get(t, k) { traps++; return t[k] } })
console.log("proxy", read(proxy), read(a), read(proxy), read(b), traps)
function length(o: any): any { return o.length }
console.log("exotics", length(Buffer.from("abc")), length(new Uint8Array(4)), length([1, 2]))
const params: any = new URLSearchParams("a=1&b=2")
function size(o: any): any { return o.size }
console.log("params", size(params), size(params))
// A cached getter still collects and receives the original, live receiver.
let collectingCalls = 0
const collecting: any = { get answer() {
  collectingCalls++
  const result: any = { id: 0, label: "" }
  result.id = this.id; result.label = this.label
  return result
} }
const ca: any = Object.create(collecting), cb: any = Object.create(collecting)
ca.id = 7; ca.label = "a"; cb.pad = true; cb.id = 17; cb.label = "b"
sum = 0
for (let i = 0; i < 40000; i++) sum += read(ca).id + read(cb).id
console.log("collecting", sum, collectingCalls, read(ca).label, read(cb).label)
// Main's deeper accessor proofs must survive alternating ordinary answers.
const getterHop: any = Object.create(g)
const getterDeep: any = Object.create(getterHop)
getterDeep.id = 61
sum = 0
for (let i = 0; i < 100; i++) sum += read(getterDeep) + read(b) + read(x)
console.log("deep-getter-alternating", sum)
Object.defineProperty(getterHop, "answer", { value: null, writable: true, configurable: true })
console.log("deep-getter-null-shadow", read(getterDeep), read(x))
getterHop.answer = undefined
console.log("deep-getter-undefined-shadow", read(getterDeep), read(x))
delete getterHop.answer
console.log("deep-getter-delete-shadow", read(getterDeep))
Object.setPrototypeOf(getterHop, { get answer() { return this.id + 100 } })
console.log("deep-getter-relink", read(getterDeep), read(x), read(b))
