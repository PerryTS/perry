### Fixed: `arguments` inside a class constructor now reflects the call site (#10484)

The `arguments` object inside a class constructor didn't match the call: constructing
through a runtime value (a class stored in a variable, returned from a function, or
imported — including every CommonJS class) saw an **empty** `arguments`, and a static
`new C(x)` reported the constructor's **declared** parameter count instead of the
number of arguments actually passed. undici's `new Request(url)` and whatwg-url's
`new URL(href)` both threw spurious "argument required" errors as a result.

**Root cause**, three layers: HIR padded a `new`-site's argument list with `undefined`
up to the declared arity before packing `arguments`, making a caller-omitted argument
indistinguishable from a declared one; the four runtime dynamic-construct paths (super-
apply, flat-ctor replay, class-object/registered-class replay) packed the synthesized
`arguments` slot like a user `...rest` parameter (only the arguments past the declared
count, empty for the common case); and codegen read a constructor's trailing-array
layout off its last declared parameter only, which misses every capturing constructor
— i.e. every CommonJS class, since Perry adds capture params mechanically.

**Fix.** HIR skips arity padding for a constructor that reads `arguments`. The four
runtime dynamic-construct sites now share one helper, `constructor_user_arg_slots`,
that packs `arguments` from every call argument. Codegen gains a `CtorAbi` (param
count, has-rest, has-synthetic-arguments) computed from the constructor's actual
layout instead of its last parameter, threaded through constructor-contract
resolution, imported-class metadata, and cross-module `new`-site argument marshaling.

**Validation.** New gap test `test_gap_10484_class_constructor_arguments.ts` (with an
undici-`Request`-shaped CommonJS fixture) fails on the base commit and passes here,
byte-identical to Node. A 45-test constructor/ABI regression sweep and the full
`perry-runtime`/`perry-codegen` unit suites are clean. Static construction and
value-construction without `arguments` usage are unaffected in instruction-count
perf; value-construction with an `arguments`-reading constructor — the case the bug
was actually about — costs roughly 20% more instructions on that specific path, a
correctness-necessitated cost of materializing a real `arguments` array where the
buggy path previously built nothing.
