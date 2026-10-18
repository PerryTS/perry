// Object model §3.6: carried prototype layouts with module-local
// `Symbol(…)` keys (registered when the prototype is bound), and reads of
// inherited keys on `Object.create(P)` family objects through the chain's
// layouts; every change to a prototype after the births.
import { key as otherKey, OtherProto, other } from "./_helpers/create_family_local_symbol/other.ts"

const key = Symbol("schema")
const second = Symbol("schema")
const brand = Symbol()
const log: string[] = []

const P: any = { _tag: "Some", [key]: true, step(x: number) { return x + 1 }, [brand]: 7, last: "p" }
const Q: any = Object.assign(Object.create(P), { kind: "q", [second]: "two" })

function syms(o: any) {
  return Object.getOwnPropertySymbols(o).map((s) => String(s) + "=" + String(o[s])).join(",")
}

log.push("P syms " + syms(P) + " keys " + Reflect.ownKeys(P).length)
log.push("Q syms " + syms(Q) + " names " + JSON.stringify(Object.getOwnPropertyNames(Q)))
log.push("same desc distinct " + String(key === second) + " " + String(key === otherKey))
log.push("P[second] " + String(P[second]) + " P[otherKey] " + String(P[otherKey]))

function run(label: string) {
  const a = Object.create(P)
  a.value = 7
  const b = Object.create(Q)
  b.value = 8
  let sum = 0
  let err = ""
  for (let i = 0; i < 5; i++) {
    try {
      sum += a.step(i)
    } catch (e) {
      err += "a" + (e as Error).constructor.name
    }
    try {
      sum += b.step(i)
    } catch (e) {
      err += "b" + (e as Error).constructor.name
    }
    if (a[key]) sum += 100
  }
  log.push(label + " sum=" + sum + " err=" + err + " tag=" + a._tag + "/" + b._tag + " last=" + a.last + "/" + b.last + " kind=" + b.kind + " brand=" + String(a[brand]))
}

run("born")
P.step = function (x: number) { return x * 2 }
run("overwritten")
P[Symbol("late")] = 1
run("symbol added")
delete P[key]
run("symbol deleted")
log.push("P syms after " + syms(P))
const k2 = Symbol("schema")
P[k2] = "rebound"
run("rebound")
log.push("P syms rebound " + syms(P))
delete P.step
run("step deleted")
Q.step = (x: number) => x - 1
run("Q shadows")
Object.setPrototypeOf(Q, { step: () => 1000, last: "z" })
run("Q relinked")

// The top level: the same reads in the module's own body.
const top = Object.create(P)
top.value = 1
log.push("top " + String(top[key]) + " " + String(top[k2]) + " " + top._tag + " " + top.last)

// A function-local `Symbol(…)` makes a key the compiler does not carry.
function local() {
  const k = Symbol("schema")
  const L: any = { [k]: 1, step(x: number) { return x } }
  const o = Object.create(L)
  o.value = 2
  return syms(L) + " " + o.step(3)
}
log.push("local " + local())
log.push("other " + other(2) + " " + syms(OtherProto))

// Collections after the births: the layouts keep their symbol keys current.
const gk = Symbol("gc")
const G: any = { [gk]: 1, g: "g", step(x: number) { return x + 3 } }
let junk: any[] = []
for (let i = 0; i < 200000; i++) {
  junk.push({ i, s: "x" + i })
  if (junk.length > 1000) junk = []
}
const go = Object.create(G)
go.value = 1
let gs = 0
for (let i = 0; i < 5; i++) gs += go.step(i)
log.push("gc " + syms(G) + " " + gs + " " + go.g + " " + String(go[gk]) + " " + syms(P) + " " + syms(Q))
run("after gc")

for (const line of log) console.log(line)
