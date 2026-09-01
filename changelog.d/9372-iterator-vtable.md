**fix(hir): keep the `Symbol.iterator` vtable entry alongside #9226's own-key registration**

`const R = class { *[Symbol.iterator]() { … } }` threw
`TypeError: value is not iterable`.

#9226 replaced `methods.push(wrapper)` with `computed_members.push(…)` in both
the class-declaration and class-expression paths. Those are not equivalent: the
first installs an instance **vtable** entry, the second registers a **prototype
own key**. `synthesize_symbol_iterator_wrapper` exists specifically to provide
the vtable entry (#5128) — without it the class carries no `@@iterator`, so the
runtime's class-registry lookup finds nothing.

Both registrations are required and the fix does both, so #9226's own-key
enumeration is preserved rather than traded away.

Verified by bisect on an isolated host (dev profile, same toolchain):

| commit | `issue_5128_user_symbol_iterator` |
|---|---|
| `015ec5fe1c` (parent) | 3 passed |
| `8b2cfe6e7b` (#9226) | 2 passed, **1 failed** |
| `8b2cfe6e7b` + this fix | **3 passed** |

and #9226's own gap test (`test_gap_9226_class_prototype_own_keys.ts`) still
matches Node byte-for-byte with the fix applied — the own-key win is intact.
