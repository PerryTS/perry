// Object model step 1: `const a = Object.create(P)` followed by stores
// `a.k = v` is a family born with its keys. Nothing in this file defines,
// freezes or relinks, so the census proves the adds free and the family
// stores slots: every observable result must still be the ordinary one.

const Tag = "~test/Tag"
const Pipe = { pipe() { return "pipe" }, describe() { return "d" } }
const Common: any = {
  [Tag]: { marker: 1 },
  ...Pipe,
  [Symbol.iterator]() { return [1, 2][Symbol.iterator]() },
}
const Some: any = Object.assign(Object.create(Common), {
  _tag: "Some",
  show(this: any) { return "some(" + this.value + ")" },
})
const Bare: any = Object.create(null)

function some(v: any) {
  const a = Object.create(Some)
  a.value = v
  return a
}

function pair(x: number, y: number) {
  const p = Object.create(Some)
  p.x = x
  p.y = y
  p.x = x + 100
  return p
}

function bare(k: number) {
  const b = Object.create(Bare)
  b.k = k
  b.j = k * 2
  return b
}

function interleaved(i: number) {
  const o = Object.create(Some)
  o.first = i
  o["dyn" + (i % 2)] = i
  o.second = i
  return o
}

function deleted(i: number) {
  const o = Object.create(Some)
  o.first = i
  delete o.first
  o.second = i
  o.first = i + 1
  return o
}

function conditional(c: boolean) {
  const o = Object.create(Some)
  if (c) o.late = 1
  o.early = 2
  o.late = 3
  return o
}

let sum = 0
for (let i = 0; i < 2000; i++) {
  const s = some(i)
  sum += s.value
  if (Object.getPrototypeOf(s) !== Some) throw new Error("proto")
}
const s = some("v")
console.log(sum, s.show(), s._tag, s.pipe(), [...s].join(","), Tag in s, s[Tag].marker)
console.log(Object.keys(s).join(","), JSON.stringify(s), Object.hasOwn(s, "value"), Object.hasOwn(s, "_tag"))
const p = pair(1, 2)
console.log(Object.keys(p).join(","), p.x, p.y, JSON.stringify(Object.entries(p)))
const b = bare(4)
console.log(Object.keys(b).join(","), b.k, b.j, Object.getPrototypeOf(Object.getPrototypeOf(b)), "toString" in b)
for (let i = 0; i < 3; i++) console.log(Object.keys(interleaved(i)).join(","))
console.log(Object.keys(deleted(5)).join(","), JSON.stringify(deleted(6)))
console.log(Object.keys(conditional(true)).join(","), Object.keys(conditional(false)).join(","))
console.log(JSON.stringify(conditional(true)), JSON.stringify(conditional(false)))
