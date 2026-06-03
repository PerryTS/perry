# Function Detached Helper References

DeepWiki was queried for `engine262/engine262` and `boa-dev/boa`; the full responses are saved next to this file:

- `reference/deepwiki/function-detached-helpers-engine262.md`
- `reference/deepwiki/function-detached-helpers-boa.md`

Useful implementation notes:

- Both engines model bound functions as callable exotic objects with target function, bound receiver, and bound argument slots.
- `Function.prototype.bind` rejects non-callable targets, creates a callable bound object, sets `length` to `max(target.length - boundArgs.length, 0)`, and sets `name` with a `"bound "` prefix.
- Bound function invocation prepends bound arguments and delegates to the target with the bound receiver. This is the behavior that makes `Function.prototype.call.bind(Object.prototype.hasOwnProperty)` work.
- Built-in functions such as `Array.isArray`, `Object.defineProperty`, and `Object.getOwnPropertyDescriptor` are ordinary callable values for `IsCallable` purposes when detached from their constructor object.
- Function `name` and `length` descriptors are non-writable, non-enumerable, and configurable for ordinary and bound functions.
