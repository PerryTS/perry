// Function.prototype.apply/call on function receivers: builtin entries, the
// arguments forward, async/generator kinds, overrides, and folds under a
// program-written Function.prototype method.
import EE from "node:events"
import * as path from "node:path"
const out: string[] = []
const log = (...a: any[]) => out.push(a.map(x => typeof x === "object" && x !== null ? JSON.stringify(x) : String(x)).join(" "))
function who(this: any, ...a: any[]) { return [this === undefined ? "undef" : this === null ? "null" : typeof this === "object" ? (this.tag ?? "obj") : typeof this, a.length, ...a] }
function sloppyWho(this: any) { return typeof this }
const o = { tag: "o" }
// apply with array-likes
log(who.apply(o, [1, 2]))
log(who.apply(o, { length: 2, 0: "a", 1: "b" } as any))
log(who.apply(o, { length: 3, 1: "x" } as any))
log(who.apply(o, "ab" as any === "ab" ? [] : []))
log(who.apply(null, [1]), who.apply(undefined, [2]), who.apply(o))
log(who.apply(o, null as any), who.apply(o, undefined))
log(who.call(o), who.call(o, 1), who.call(o, 1, 2, 3, 4), who.call(null, 5), who.call(undefined))
log(who.apply(7 as any, [1]))
const ta = new Int8Array([3, 4]); log(who.apply(o, ta as any))
const sparse: any[] = [1, , 3]; log(who.apply(o, sparse))
const getterList = { length: 1, get 0() { return "got" } }; log(who.apply(o, getterList as any))
try { who.apply(o, 5 as any) } catch (e: any) { log("TypeError?", e instanceof TypeError) }
// arguments forwarding
function fwd(this: any, ..._r: any[]) { return function (this: any) { return who.apply(this, arguments) } }
const f1: any = fwd(); log(f1.call(o, 1, 2), f1(3), f1.apply(null, [4, 5, 6]))
function mut(this: any, a: any, b: any) { arguments[0] = "m"; return who.apply(this, arguments) }
log(mut.call(o, 1, 2))
function mutlen(this: any, a: any, b: any) { (arguments as any).length = 1; return who.apply(this, arguments) }
log(mutlen.call(o, 1, 2))
function sloppyFwd(this: any) { return sloppyWho.apply(this, arguments) }
log(sloppyFwd.call(undefined), sloppyFwd.call(3))
// overwritten own apply / call
const g: any = function (this: any, x: any) { return ["g", x] }
g.apply = function (t: any, l: any) { return ["own-apply", l && l.length] }
g.call = function (t: any, x: any) { return ["own-call", x] }
function viaArgs(this: any) { return g.apply(this, arguments) }
log(g.apply(o, [1]), g.call(o, 2), viaArgs(9, 8))
// bound functions
const b = who.bind(o, "bound")
log(b.apply({ tag: "ignored" }, [1]), b.call(null, 2))
function viaB(this: any) { return b.apply(this, arguments) }
log(viaB(3, 4))
// proxy function
const p: any = new Proxy(who, { apply(t, th, args) { return ["proxied", args.length] } })
log(p.apply(o, [1, 2]), p.call(o, 1))
function viaP(this: any) { return p.apply(this, arguments) }
log(viaP(1, 2, 3))
const p2: any = new Proxy(who, { get(t, k) { return k === "apply" ? () => "trap-apply" : (t as any)[k] } })
log(p2.apply(o, [1]))
// class constructor via call: TypeError
class K { }
try { (K as any).call(o) } catch (e: any) { log("class call TypeError", e instanceof TypeError) }
try { (K as any).apply(o, []) } catch (e: any) { log("class apply TypeError", e instanceof TypeError) }
// native receivers
log(Math.max.apply(null, [1, 5, 3]), Math.max.call(null, 1, 9), String.prototype.toUpperCase.call("ab"))
log(Array.prototype.join.call({ length: 2, 0: "x", 1: "y" }, "-"), Array.prototype.slice.apply([1, 2, 3], [1]))
const e = new EE(); let hits = 0; e.on("x", (v: any) => { hits += v })
log(EE.prototype.emit.call(e, "x", 2), EE.prototype.emit.apply(e, ["x", 3]), hits)
function viaEmit(this: any) { return EE.prototype.emit.apply(this, arguments) }
log(viaEmit.call(e, "x", 5), hits)
// generator / async function receivers
function* gg(this: any, a: any) { yield [this && this.tag, a] }
async function af(this: any, a: any) { return [this && this.tag, a] }
function viaG(this: any) { return (gg as any).apply(this, arguments).next().value }
log(gg.apply(o, [5]).next().value, gg.call(o, 6).next().value, viaG.call(o, 7))
const gOwn: any = function* () { yield 1 }; gOwn.apply = () => "own-gen-apply"; log(gOwn.apply(null, []))
const GF: any = Object.getPrototypeOf(gg), AF: any = Object.getPrototypeOf(af)
GF.apply = function () { return "gf-apply" }; AF.call = function () { return "af-call" }
function viaGRaw(this: any) { return (gg as any).apply(this, arguments) }
log(gg.apply(o, [1]), viaGRaw.call(o, 2), (af as any).call(o, 3), gg.call(o, 4).next().value)
delete GF.apply; delete AF.call
log(gg.apply(o, [8]).next().value, viaG.call(o, 9))
af.call(o, 10).then((v: any) => console.log("async", JSON.stringify(v)))
// Function.prototype.apply/call overwritten (last, it is global)
const origApply = Function.prototype.apply, origCall = Function.prototype.call
;(Function.prototype as any).apply = function (this: any, t: any, l: any) { return ["patched-apply", l ? l.length : -1] }
;(Function.prototype as any).call = function (this: any, t: any, x: any) { return ["patched-call", x] }
log(who.apply(o, [1, 2]), who.call(o, 7), f1(1, 2), viaB(1), Math.max.apply(null, [1]), EE.prototype.emit.call(e, "x", 1))
log(Array.prototype.join.call([1, 2], "-"), String.prototype.toUpperCase.call("ab"), (function (this: any, x: any) { return x }).call(null, "iife"), Math.max.call(null, 4), Object.prototype.hasOwnProperty.call(o, "tag"))
log(path.join.apply(null, ["a", "b"]), path.join.call(null, "a", "c"), Promise.resolve.call(Promise, 1) instanceof Promise)
;(Function.prototype as any).apply = origApply; (Function.prototype as any).call = origCall
log(who.apply(o, [1, 2]), who.call(o, 7), f1(1, 2))
console.log(out.join("\n"))
