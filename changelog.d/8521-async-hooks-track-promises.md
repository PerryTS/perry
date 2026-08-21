### fix(async_hooks): honor `trackPromises` selection

`createHook()` now validates Node's `trackPromises` option and rejects its
invalid combination with `promiseResolve`. Hooks that set `trackPromises` to
`false` no longer allocate or receive Promise lifecycle events, including
when they are enabled beside hooks that continue tracking Promises. Advances
#6764.
