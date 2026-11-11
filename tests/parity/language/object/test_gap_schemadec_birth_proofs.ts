// Distinct prototypes can share their key layout; their birth identity cannot.
const first: any = { inherited: 1 }
const second: any = { inherited: 2 }
let sum = 0
for (let i = 0; i < 200; i++) {
  const proto = i % 2 === 0 ? first : second
  const object: any = Object.create(proto)
  object.value = i
  sum += object.inherited + object.value
  if (Object.getPrototypeOf(object) !== proto) throw new Error("prototype identity")
}
console.log(sum)
// A completed prototype mark must preserve later descriptor and link changes.
const third: any = { inherited: 10 }
Object.setPrototypeOf(first, third)
Object.defineProperty(first, "input", {
  set(value) { this.received = value + this.inherited }, configurable: true
})
const result: any = Object.create(first)
result.input = 5
console.log(result.received, Object.hasOwn(result, "input"), result.inherited)
Object.defineProperty(first, "inherited", { value: 7 })
const later: any = Object.create(first)
later.input = 5
console.log(later.received, later.inherited)
console.log(Object.getPrototypeOf(Object.create(null)) === null)
