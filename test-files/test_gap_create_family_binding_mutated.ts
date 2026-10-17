// Object model step 1: `const a = Object.create(P)` families when the program
// can change a prototype or the object: accessors and read-only keys on the
// chain, a definition after the birth, an integrity operation and a relink
// must all keep the ordinary [[Set]].

const log: string[] = []
const Base: any = { kind: "base" }
const Proto: any = Object.defineProperty(Object.assign(Object.create(Base), { tag: "p" }), "guarded", {
  set(v: any) { log.push("guarded:" + v) },
  get() { return "g" },
})
const ReadOnly: any = Object.defineProperty(Object.create(Base), "fixed", { value: 1, enumerable: true })

function withSetter(v: any) {
  const a = Object.create(Proto)
  a.guarded = v
  a.plain = v
  return a
}

function readOnly(v: any) {
  const a = Object.create(ReadOnly)
  try { a.fixed = v } catch (e) { log.push("fixed:" + (e instanceof TypeError)) }
  a.other = v
  return a
}

function defineAfter(v: any) {
  const a = Object.create(Proto)
  Object.defineProperty(a, "locked", { value: 0, writable: false, enumerable: true })
  try { a.locked = v } catch (e) { log.push("locked:" + (e instanceof TypeError)) }
  a.free = v
  return a
}

function frozen(v: any) {
  const a = Object.create(Proto)
  Object.preventExtensions(a)
  try { a.extra = v } catch (e) { log.push("extra:" + (e instanceof TypeError)) }
  return a
}

function plain(v: any) {
  const a = Object.create(Proto)
  a.later = v
  a.plain = v
  return a
}

const w = withSetter(1)
console.log(Object.keys(w).join(","), w.guarded, w.plain)
const r = readOnly(2)
console.log(Object.keys(r).join(","), r.fixed, r.other)
const d = defineAfter(3)
console.log(Object.keys(d).join(","), d.locked, d.free)
const f = frozen(4)
console.log(Object.keys(f).join(","), Object.isExtensible(f))

// The prototype changes after births: a setter for `later` on the chain,
// found through an object, then a relink of the prototype itself.
console.log(Object.keys(plain(5)).join(","))
Object.defineProperty(Object.getPrototypeOf(Object.getPrototypeOf(plain(0))), "later", {
  set(v: any) { log.push("later:" + v) },
  configurable: true,
})
const q = plain(6)
console.log(Object.keys(q).join(","), q.later)
Object.setPrototypeOf(Proto, { later: "data" })
const t = plain(7)
console.log(Object.keys(t).join(","), t.later, Object.getPrototypeOf(Object.getPrototypeOf(t)).later)
Object.freeze(Proto)
const u = plain(8)
console.log(Object.keys(u).join(","), u.later)
console.log(log.join(" "))
