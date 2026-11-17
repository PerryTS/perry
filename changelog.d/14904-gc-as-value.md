A value read of the `gc` global (`const g = gc; g()`) is the `globalThis.gc` function. It lowered to a reference to the extern function and printed `true`; a call `gc()` is unchanged.
