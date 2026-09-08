Direct named imports from `node:async_hooks` now expose the real
`AsyncLocalStorage` and `AsyncResource` prototype methods (#9980). Named
callable imports could be materialized before the namespace-object path
installed async_hooks' constructor-decoration registry row, so the canonical
closures were cached without their prototype methods. The callable export path
now installs the module row before minting either constructor.
