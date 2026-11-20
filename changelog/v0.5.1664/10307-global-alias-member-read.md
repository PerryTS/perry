Property reads through the `global` alias now see what user code installed.

`global` is `globalThis` under another name, a self-reference installed by
`populate_global_this_builtins`, so it must be exempt from the `<name>.<name>`
undo exactly as `globalThis` is. Collapsing `global.<x>` to the intrinsic
`GlobalGet(0).<x>` static surface answered `undefined` for every property user
code had installed: `global.foo = 1; global.foo` read back `undefined` while
`globalThis.foo` saw it, because only the read collapsed. `@opentui/core` does
`global.window = {}` and then `global.window.requestAnimationFrame = …`, which
threw "Cannot set properties of null or undefined".
