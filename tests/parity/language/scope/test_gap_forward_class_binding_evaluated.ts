// A closure created BEFORE a function-nested class declaration reads the class
// through its lexical binding when it runs. For a class lowered per evaluation
// (unresolvable heritage such as Array/Map/Error, computed field keys, private
// elements) that binding is the evaluated class object, not the shared template.
// fuzzysort's UMD factory shape: `go` (declared first) does `new KeysResult(n)`
// for `class KeysResult extends Array { get ['score']() {...} }` declared later.

const lib = (() => {
  const makeKeys = (n: number) => { const r: any = new KeysResult(n); r._score = 0.5; return r }
  const makeMap = () => new Registry()
  const makeError = (m: string) => new CodedError(m)
  const isCoded = (e: unknown) => e instanceof CodedError
  const makeField = () => new WithField()
  const makePrivate = () => new Counter()
  const makeChild = () => new SubKeys(1)
  const keysClass = () => KeysResult
  class KeysResult extends Array {
    get ["score"]() { return (this as any)._score * 10 }
    first() { return this[0] }
  }
  class SubKeys extends KeysResult { tag() { return "sub" } }
  class Registry extends Map<string, number> { total() { let t = 0; for (const v of this.values()) t += v; return t } }
  class CodedError extends Error { code() { return "E_CODED" } }
  const key = "dyn"
  class WithField { [key] = 7; read() { return (this as any).dyn } }
  class Counter { #n = 0; inc() { return ++this.#n } }
  return { makeKeys, makeMap, makeError, isCoded, makeField, makePrivate, makeChild, keysClass, KeysResult, Registry, CodedError }
})()

const k = lib.makeKeys(2)
k[0] = "a"
console.log("keys", k.length, k.score, k.first(), k instanceof lib.KeysResult, Array.isArray(k))
console.log("keys proto", Object.getPrototypeOf(k) === lib.KeysResult.prototype, lib.keysClass() === lib.KeysResult)
const m = lib.makeMap(); m.set("a", 2); m.set("b", 3)
console.log("map", m.total(), m instanceof lib.Registry, m instanceof Map)
const e = lib.makeError("boom")
console.log("error", e.message, Object.getPrototypeOf(e) === lib.CodedError.prototype, e instanceof lib.CodedError, e instanceof Error, lib.isCoded(new lib.CodedError("x")))
console.log("field", lib.makeField().read())
const c = lib.makePrivate(); c.inc()
console.log("private", c.inc())
const s = lib.makeChild()
console.log("child", s.tag(), typeof s.first, s instanceof lib.KeysResult)

// Called before the declaration ran: TDZ, as in node.
function early() {
  const touch = () => new Late()
  let msg = "none"
  try { touch() } catch (err) { msg = (err as Error).constructor.name }
  class Late extends Array {}
  return msg + " " + (touch() instanceof Late)
}
console.log("tdz", early())
