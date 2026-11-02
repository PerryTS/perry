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

`crates/perry-hir/tests/fluent_chain_lowering.rs`'s
`native_fluent_chain_still_dispatches_through_native_methods` asserted the
pre-fix, ambient/no-import, spelling-based dispatch this change deliberately
tightens (a bare `new Decimal(1)` with no import now correctly falls through
to an unresolved-global reference, matching Node's `ReferenceError`, instead
of silently reaching the native handle). That test predates this change and
was never updated for it, so it went red on this same commit without this
diff touching its file — caught by the sweep's `cargo test --workspace`,
not by any diff-scoped gate. Removed here, with the rationale recorded
inline, rather than left for a descendant PR to patch around a third time.
