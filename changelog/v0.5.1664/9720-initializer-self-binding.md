**A closure in a `let`/`const` declarator's own initializer can now see the
binding that declarator introduces** — `const off = ev.on(() => off())`,
`const { unmount } = await render({ onDone: () => unmount() })`. It previously
threw `ReferenceError: <name> is not defined` (#9718).

The function-body forward-capture pre-pass (`pre_register_forward_captured_lets`)
decides whether a `let`/`const` binding needs a boxed, TDZ-seeded forward
declaration by asking whether any closure *seen so far* references its name. It
recorded a declarator's own initializer into that set only **after** making that
decision — which is all the later-declarator case needs (`let z = (w) => { … O … },
O = setTimeout(z, K)`, the minified `new Promise` executor shape), but left the
self-referential shape unregistered. The reference then fell through to
`js_global_get_or_throw_unresolved`. Recording the initializer's closure refs
before the decision is a strict superset: later declarators still see them.

The hole was reachable from every declarator form, because the pre-pass is what
the destructuring path relies on — `destructuring/var_decl.rs`'s own
`is_function_expr_init` pre-registration covers only simple `Pat::Ident`
bindings, and its `ast_expr_contains_function_expr` scan does not descend
through `await`. So `const O = await mk({ cb: () => O() })` (plain binding
behind an `await`), every object/array/nested pattern, and `{ key }` shorthand
all failed; only a closure created *after* the declaration worked.

Found in claude-code: the `install` subcommand is

```js
let { unmount: O } = await eB(el(wMA, { onDone: (w, $) => { O(), q(w, $) }, … }))
```

so `claude install <bad-channel>` threw an uncaught `ReferenceError: O is not
defined` mid-teardown, losing the final newline and the exit code — perry exited
0 where node exits 1 (the `B60_install_badtarget` divergence carried in #9575).

`test-files/test_gap_9718_initializer_self_binding.ts` pins all of it against
node: plain binding, object pattern, `{ key }` shorthand, array pattern, nested
pattern, `let` and `const`, with and without `await`, plus a multi-declarator
statement so the reordered scan's pre-existing earlier-refs-later behavior stays
covered. Unpatched, 7 of its 12 lines are `ReferenceError`; patched, the output
is byte-identical to node 26.5.1.
