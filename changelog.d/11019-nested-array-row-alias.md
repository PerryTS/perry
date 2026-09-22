Fixed module-level constant nested arrays returning `undefined`, serving stale
elements, and dropping compound assignments when an inner row became observable.
The flat constant table optimization now admits only direct element reads and
read-only row aliases; returned, compared, passed, logged, or mutated rows keep
the normal JavaScript heap representation. Read-only convolution-style kernels
continue to use the flat table fast path.
