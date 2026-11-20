### fix(perf_hooks): give public classes Node-compatible prototypes and constructors

The public `node:perf_hooks` classes now expose their real prototype chains,
property descriptors, accessors, methods, and `Symbol.toStringTag` values.
Performance entries, observers, the global `performance` object, and observer
entry lists are linked to the same canonical prototypes, so reflection,
`instanceof`, extracted prototype calls, and receiver validation agree with
Node.

`new PerformanceMark(name, options)` now creates a detached mark with cloned
detail data without adding it to the performance timeline. The other
non-publicly-constructible perf classes throw `ERR_ILLEGAL_CONSTRUCTOR`, while
`PerformanceObserver` retains its supported constructor. Entry-list filters
also report Node-compatible missing-argument, Symbol-coercion, and invalid-this
errors across both generic and typed dispatch paths. Fixes #8231.
