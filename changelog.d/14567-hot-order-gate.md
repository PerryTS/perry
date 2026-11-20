CI now keeps the runtime's default hot-text order honest. Every PR checks
that each runtime function the order lists still exists in the release
archives (`scripts/hot_text_order.py check --names-only`, a second in
`gc-call-effects-linux`), so a renamed core-tier function fails instead of
silently dropping out of the hot run and costing peak RSS. The full tier runs
the whole check, including the callgrind coverage of a smoke program, in
`binary-size`. The `function_order` integration test is true again: it finds a
small function's code in its callable wrapper when the public entry has been
inlined, and leaves the default order's bound symbols out of its symbol
comparison.
