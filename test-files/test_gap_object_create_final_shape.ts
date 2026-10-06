// Prototype identity, descriptor definitions, later transitions and exotic hops.
const proto: any = { inherited: 17, read() { return this.x } }
const a: any = Object.create(proto)
const b: any = Object.create(proto)
console.log(a !== b, Object.getPrototypeOf(a) === proto, a.inherited)
a.x = 23
for (let i = 0; i < 20; i++) a['k' + i] = i
console.log(a.read(), a.k19, Object.getPrototypeOf(a) === proto)
const p2 = { inherited: 41 }
Object.setPrototypeOf(a, p2)
console.log(Object.getPrototypeOf(a) === p2, a.inherited, b.inherited)
const bare: any = Object.create(null)
bare.x = 7
console.log(Object.getPrototypeOf(bare) === null, 'toString' in bare, bare.x)
Object.setPrototypeOf(bare, proto)
console.log(Object.getPrototypeOf(bare) === proto, bare.read())
let writes = 0
const d: any = Object.create(proto, {
  x: { value: 29, writable: true, enumerable: true, configurable: true },
  accessor: { get() { return this.x + 1 }, set(v: number) { writes++; this.x = v }, enumerable: false, configurable: true }
})
console.log(Object.getPrototypeOf(d) === proto, d.accessor, Object.keys(d).join(','))
d.accessor = 31
const desc = Object.getOwnPropertyDescriptor(d, 'accessor')!
console.log(d.x, writes, typeof desc.get, typeof desc.set, desc.enumerable, desc.configurable)
const inheritedSetter: any = { set x(v: number) { this.saved = v } }
const s: any = Object.create(inheritedSetter)
s.x = 43
console.log(s.saved, Object.hasOwn(s, 'x'))
let traps = 0
const proxy: any = new Proxy({ inherited: 53 }, { get(t, k, r) { traps++; return Reflect.get(t, k, r) } })
const child: any = Object.create(proxy)
console.log(Object.getPrototypeOf(child) === proxy, traps, child.inherited, traps)
for (const p of [[], new Uint8Array(2), function f() {}]) {
  const o = Object.create(p)
  o.own = 61
  console.log(Object.getPrototypeOf(o) === p, o.own)
}
for (const p of [undefined, 1, 'x', true, Symbol('x')]) {
  try { Object.create(p); console.log('accepted') } catch (e) { console.log(e instanceof TypeError) }
}
