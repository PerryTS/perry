// Run with PERRY_FULL_OUTLINE_IC=1 as well as the default inline lowering.
function read(o: any): any { return o.value }
const a: any = { value: 1 }
const b: any = { pad: 0, value: 2 }
const c: any = { value: 3, tail: 0 }
let sum = 0
for (let i = 0; i < 30; i++) {
  sum += read(a) + read(b) + read(c)
}
console.log("ways", sum)
const holder: any = { value: 11 }
const inherited: any = Object.create(holder)
for (let i = 0; i < 5; i++) console.log("holder", read(inherited))
holder.value = 12
console.log("updated", read(inherited))
inherited.value = 13
console.log("shadow", read(inherited))
delete inherited.value
console.log("unshadow", read(inherited))
Object.defineProperty(holder, "value", { get() { return 14 }, configurable: true })
console.log("accessor", read(inherited), read(inherited))
delete holder.value
console.log("deleted", read(inherited))
Object.setPrototypeOf(inherited, { value: 15 })
console.log("relinked", read(inherited))
let traps = 0
const proxy = new Proxy({ value: 16 }, { get(target, key) { traps++; return target[key] } })
console.log("proxy", read(proxy), read(proxy), traps)
class Tagged { readonly _tag = "Objects" }
function tag(o: any): any { return o._tag }
const tagged: any = new Tagged()
console.log("class-field", tag(tagged), tag(tagged), Object.hasOwn(tagged, "_tag"))
tagged._tag = "Changed"
console.log("class-update", tag(tagged))
function length(o: any): any { return o.length }
console.log("exotic-length", length(Buffer.from("abc")), length(new Uint8Array(4)), length([1, 2]))
const params: any = new URLSearchParams("a=1&b=2")
function size(o: any): any { return o.size }
console.log("params", size(params), size(params))
