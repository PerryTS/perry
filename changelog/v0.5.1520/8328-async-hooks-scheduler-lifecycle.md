### fix(async_hooks): track scheduler resource lifecycles

`AsyncHook` handles now retain working `enable()` and `disable()` methods when
their static type is lost, and hook callback snapshots remain rooted across
moving garbage collections. Timers, immediates, intervals, microtasks, and
next-tick callbacks now carry stable async resource identities and emit their
expected lifecycle events. Advances #6764.
