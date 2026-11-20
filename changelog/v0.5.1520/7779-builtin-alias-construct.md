**Constructing a builtin through a variable alias now produces a real instance** (#7524).

```ts
const ET = EventTarget;
typeof new ET().addEventListener   // was "undefined", now "function"
```

`new EventTarget()` written directly is lowered by codegen straight to the
factory, so it was always correct — only the indirect shapes were broken. An
alias routes through the `globalThis` value instead, whose closure is the shared
`global_this_builtin_noop_thunk`: it allocates a bare object and never stamps the
class id or attaches the per-kind state, so the #6301 prototype-chain fallback
had nothing to resolve against and the instance came back with no surface.

`EventTarget`, `AbortController`, `TextEncoder`, `URLSearchParams` and
`DisposableStack` now dispatch to the same factory the direct form uses, so the
two forms agree on behaviour and not merely on shape — `test_gap_builtin_alias_construct_7524.ts`
asserts the aliased `TextEncoder` really encodes and the aliased
`URLSearchParams` really parses its init string.

The arms live in a new `class_registry/builtin_alias_construct.rs` because
`construct.rs` sat at 1999 lines against the 2000-line CI cap and had no room for
any new arm. The existing `Map`/`Set`/`WeakMap`/`WeakSet`/`WeakRef` arms moved
there verbatim alongside them — the same category, described by their own comment
as "the constructor was obtained as a value". No `cfg`-gated arm moved: the
delegation is a guard arm, so a name claimed while its body is compiled out would
return `undefined` instead of falling through to the class-object path.

Still open on #7524: subclassing a native base (`class A extends AbortController {}`)
yields an empty surface via a separate per-builtin mechanism, and `FormData` is
owned by perry-stdlib, which perry-runtime cannot call into.
