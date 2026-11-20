The runtime's own closures (promise resolving functions, combinator element functions,
`finally` thunks, await continuations, stream / events / fs helpers) store a captured
promise, closure, array or object as a tagged pointer value (`closure_set_capture_ref`,
read back with `closure_capture_ref`). These captures were bare heap addresses in
capture slots, which the collector had to recognise by guessing that an address-shaped
word is a pointer. No behaviour change.
