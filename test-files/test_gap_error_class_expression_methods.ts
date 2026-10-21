// Methods of a per-evaluation Error subclass (a class expression created in a
// factory, or a function-nested `class extends Error`) called as `e.m()` on an
// untyped receiver. opencode's NamedError.create(...) classes: `toObject()`
// returned `{}` instead of running, so every session error serialized as `{}`.
abstract class NamedError extends Error {
  abstract toObject(): { name: string; data: unknown }
  static create(name: string) {
    const result = class extends NamedError {
      public override readonly name = name
      constructor(public readonly data: unknown, options?: ErrorOptions) {
        super(name, options)
        this.name = name
      }
      toObject() { return { name, data: this.data } }
      describe(prefix: string) { return prefix + this.name }
    }
    Object.defineProperty(result, "name", { value: name })
    return result
  }
}
const Unknown = NamedError.create("UnknownError")
const Api = NamedError.create("APIError")
const errors: unknown[] = [new Unknown({ message: "x" }), new Api({ status: 500 })]
for (const e of errors) {
  const any = e as any
  console.log(JSON.stringify(any.toObject()), any.describe("> "), any instanceof NamedError, any instanceof Error)
}
function make(tag: string) {
  class Tagged extends TypeError { tag() { return tag } self() { return this } }
  return new Tagged("boom " + tag)
}
const t: any = make("t1")
console.log(t.tag(), t.self() === t, t.message, t instanceof TypeError)
try { t.missing() } catch (err) { console.log("missing", (err as Error).constructor.name) }
