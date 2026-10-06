// Shared mutable identities, recursive/TDZ bindings and lexical receivers
// must survive the fresh closure bulk initializer.
function shared(seed: number) {
  let count = seed
  let payload: any = { value: seed }
  const add = (n: number) => { count += n; payload = { value: count }; return count }
  const read = () => count + payload.value
  const another = () => { count++; return payload.value }
  return [add, read, another]
}
const a = shared(10), b = shared(10)
console.log(a[0](2), a[1](), a[2](), a[1](), b[1](), a[0] === b[0])
function recursive() {
  let f: any = (n: number): number => n === 0 ? 1 : n * f(n - 1)
  const call = () => f(5)
  console.log(call())
  f = (n: number) => n + 1
  console.log(call())
}
recursive()
function tdz() {
  const read = () => value
  try { read() } catch (e) { console.log(e instanceof ReferenceError) }
  let value = 17
  console.log(read())
}
tdz()
const perIteration: any[] = []
for (let i = 0; i < 4; i++) {
  let value = i
  perIteration.push(() => ++value, () => value)
}
console.log(perIteration.map(f => f()).join(','))
class Receiver {
  value = 7
  make() { let count = 1; return () => this.value + ++count }
}
const arrow = new Receiver().make()
console.log(arrow.call({ value: 100 }), arrow())
async function asyncCapture() {
  let value = 1
  const inc = () => ++value
  await Promise.resolve()
  console.log(inc(), value)
  return () => ++value
}
asyncCapture().then(f => console.log(f()))
function* generatorCapture() {
  let value = 10
  const inc = () => ++value
  yield inc
  yield inc
}
const gen = generatorCapture()
const first: any = gen.next().value
console.log(first(), (gen.next().value as any)(), first())
