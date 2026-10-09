Fix a non-optional link that continues an optional chain of several `?.`
links. `o?.s?.x().y`, `o?.s?.x().y()`, `o?.s?.x()()` and `o?.s?.x()[0]`
threw when `o.s` was nullish instead of evaluating to `undefined`: the
upstream chain lowers to nested short-circuit guards (with the receiver
temporaries they bind), and the continuation peeled only the outer guard, so
it read the inner guard's `undefined` result. Both the member and the call
continuation now run inside the end of the whole short-circuit spine. This is
the `options.query?.search?.trim().toLowerCase()` query in `@opentui/keymap`
that stopped OpenCode's TUI with "Cannot read properties of undefined".
