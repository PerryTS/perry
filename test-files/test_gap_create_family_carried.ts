// Object model §3.6: `Object.create(P)` families on a chain of prototypes
// born on their carried layouts (computed keys from other modules, a spread
// of another module's literal, accessors, symbol keys), in a program that
// can change prototypes (so each add takes the checked proof), and every
// change to a prototype of the chain after the births.
import * as Keys from "./_helpers/create_family_carried/keys.ts"
import { TypeId } from "./_helpers/create_family_carried/keys.ts"
import { PipeProto, Mutated } from "./_helpers/create_family_carried/base.ts"

const log: string[] = []

const CommonProto: any = {
  [TypeId]: { _A: 1 },
  ...PipeProto,
  [Symbol.iterator](this: any) {
    return [this.value][Symbol.iterator]()
  }
}

const SomeProto: any = Object.defineProperty(
  Object.assign(Object.create(CommonProto), {
    _tag: "Some",
    _op: "Some",
    [Keys.symbol](this: any, that: any) {
      return that !== null && typeof that === "object" && that._tag === "Some" && that.value === this.value
    },
    toString(this: any) {
      return `some(${this.value})`
    }
  }),
  "valueOrUndefined",
  {
    get(this: any) {
      return this.value
    }
  }
)

const MutatedProto: any = { ...Mutated, kind: "mutated" }

function some(v: any) {
  const a = Object.create(SomeProto)
  a.value = v
  return a
}

function pair(x: any, y: any) {
  const a = Object.create(MutatedProto)
  a.x = x
  a.y = y
  return a
}

function show(label: string, o: any) {
  log.push(
    label +
      " own=" + JSON.stringify(Object.getOwnPropertyNames(o)) +
      " value=" + String(o.value) +
      " tag=" + o._tag +
      " voU=" + String(o.valueOrUndefined) +
      " str=" + String(o) +
      " eq=" + o[Keys.symbol](some(o.value)) +
      " iter=" + JSON.stringify([...o]) +
      " pipe=" + o.pipe() +
      " insp=" + o[Keys.Inspect]() +
      " type=" + JSON.stringify(o[TypeId])
  )
}

// A definition with a key the census cannot name: every add in this
// program takes the checked proof.
const dynamicKey = ["dyn", String(log.length)].join("")
Object.defineProperty(globalThis, dynamicKey, { value: 1, configurable: true })

for (let i = 0; i < 3; i++) show("born" + i, some(i))
log.push("protoKeys=" + JSON.stringify(Reflect.ownKeys(SomeProto).map(String)))
log.push("commonKeys=" + JSON.stringify(Reflect.ownKeys(CommonProto).map(String)))
log.push("mutatedKeys=" + JSON.stringify(Object.keys(MutatedProto)))
const p = pair(1, 2)
log.push("pair own=" + JSON.stringify(Object.keys(p)) + " extra=" + p.extra + " m=" + p.m())

// A setter for the family key on the grandparent: adds call it.
Object.defineProperty(CommonProto, "value", {
  set(v: any) { log.push("common-set:" + v) },
  get() { return "from-common" },
  configurable: true
})
show("setter", some(10))
delete CommonProto.value
show("deleted", some(11))

// The parent frozen: it holds no `value`, the add still lands.
Object.freeze(SomeProto)
show("frozen", some(12))

// A read-only `value` on the root: the add fails (strict code throws).
Object.defineProperty(Object.prototype, "value", { value: "root", writable: false, configurable: true })
try {
  some(13)
  log.push("root-readonly: no throw")
} catch (e) {
  log.push("root-readonly: " + (e instanceof TypeError))
}
delete (Object.prototype as any).value
show("root-deleted", some(14))

// The grandparent relinked to a prototype with a setter.
const Trap = { set value(v: any) { log.push("trap-set:" + v) } }
Object.setPrototypeOf(CommonProto, Trap)
const t = some(15)
log.push("relinked own=" + JSON.stringify(Object.keys(t)))

// A writable data `x` on a carried prototype made read-only.
Object.defineProperty(MutatedProto, "x", { value: 0, writable: true, configurable: true })
log.push("pair2 own=" + JSON.stringify(Object.keys(pair(3, 4))))
Object.defineProperty(MutatedProto, "x", { writable: false })
try {
  pair(5, 6)
  log.push("readonly-x: no throw")
} catch (e) {
  log.push("readonly-x: " + (e instanceof TypeError))
}

for (const line of log) console.log(line)
