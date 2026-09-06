**A native-instance tag created by a bare assignment now follows the binding it
assigns to, not the identifier's spelling** (#9847). One
`O = childProcess.spawn(...)` in a bundled helper used to type *every* binding
named `O` in the module as a `child_process` instance.

`lower_assign` registered the tag through `push_module_native_instance`, whose
key is the identifier text and whose scope is the whole module, and its
class-name match ends in a catch-all — so any method on any native module,
assigned to a variable, claimed that name for the rest of the program. Minified
bundles reuse single letters everywhere, so the collision is the normal case,
not a corner: `cli_2.1.112.js` (claude-code) compiles as one module, contains
`let O; try { O = fA1.spawn(z.file, z.args, z.options) }`, and binds the name
`O` 5,381 more times. Among them is the `for (let { segment: O } of ... )`
binding in `string-width`, which holds a grapheme **string**; its
`O.codePointAt(0)` lowered as
`NativeMethodCall { module: "child_process", class_name: Some("Instance") }`
and reached the right answer only because native-instance dispatch falls
through to a generic path on a string receiver — once per grapheme, in the
loop that dominates a claude-code turn.

The tag is now keyed on the `LocalId` the assignment target resolves to. That
is the same correction #7775 made for `new Proxy` bindings (`proxy_locals` →
`proxy_local_ids`) and it keeps the cross-function reach the module-wide table
existed for: a module-level `let client;` assigned inside `init()` and read
inside `handler()` resolves to the same binding in both. A target that resolves
to no local at all — a bare global — still falls back to the name-keyed table,
which is strictly no worse than the previous behaviour that used it for
everything.

This is a mis-typing fix, not a wrong-answer fix: the mislowered calls already
degraded gracefully. But they degraded through a dispatch that had no business
seeing them, and a `child_process` method table that ever gained a
`codePointAt`, `length` or `test` entry would have captured string calls
silently.
