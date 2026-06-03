# `with` object-environment semantics

DeepWiki research was run against `engine262/engine262` and `boa-dev/boa`.
The raw responses are kept next to this note:

- `reference/deepwiki/with-semantics-engine262.md`
- `reference/deepwiki/with-semantics-boa.md`

Useful implementation points for c262:

- A `with` statement inserts an object environment record into the lexical
  environment chain. Closures created inside the statement retain that chain,
  so reads in the closure must still consult the object environment before the
  outer lexical fallback.
- `HasBinding(name)` first checks `HasProperty(bindings, name)`. If the record
  is a `with` environment, it then reads `bindings[Symbol.unscopables]`; when
  that value is an object and `ToBoolean(unscopables[name])` is true, the
  binding is skipped and lookup continues outward.
- `GetBindingValue(name, strict)` rechecks property existence and returns
  `Get(bindings, name)` when present. If absent, it returns `undefined` for
  non-strict object-environment binding access or throws `ReferenceError` when
  strict.
- `SetMutableBinding(name, value, strict)` rechecks property existence at set
  time. If the property disappeared and the write is strict, it throws
  `ReferenceError`; otherwise it performs ordinary object `Set`.
- `DeleteBinding(name)` delegates to object delete.
- `with` is an early syntax error in strict-mode code, but strict functions can
  be called from inside a non-strict `with` block. In that case the strict flag
  matters when the strict function performs a write resolved through the
  captured object environment.

This PR implements a scoped subset of those rules in c262 lowering/codegen:
identifier reads and writes inside a lowered `with` scope consult the captured
object environment, respect `Symbol.unscopables` on lookup, and strict writes
recheck the object binding after evaluating the RHS. Full object-environment
coverage remains larger than this parity bucket: primitive `ToObject` at
statement entry, unqualified `delete`, update expressions targeting with
bindings, and fully chained writes across nested `with` scopes are not covered.
