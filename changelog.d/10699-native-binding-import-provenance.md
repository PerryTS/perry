**A `perry.compilePackages` copy of commander, lru-cache, or decimal.js is no
longer overridden by their bundled native bindings.** `new Command()`,
`new LRUCache()`, and `new Decimal()` chained directly onto a method call
(`new Command().name(...)`, `new LRUCache(...).set(...)`,
`new Decimal(...).dividedBy(...)`) matched those class names unconditionally
and routed straight to the native handle, even when the user asked for the
real package to be compiled from source — the only way to opt out was to
rename the import. Construction and method dispatch now resolve through the
same compilePackages-aware provenance table `is_native_module` already
consults, so a compiled copy of the real package runs its own code at its
documented import name. The (unmodified) native binding still installs when
the package is not opted into `compilePackages`. Fixes #10439.
