Whole-program facts: a module namespace now resolves a named key by name, as
ECMAScript ResolveExport does: the module's own export of that name, else that
name of each `export *` source (never `default`). A name imported through a
barrel module of `export *` lines used to hold every export of every source,
so each call through it ran every function the barrel re-exports. The
TypeScript sources (tssrc), which import through `_namespaces/ts` barrels,
passed the facts work bound and got no facts at all; they now converge (work
200M+ -> 28.4M, solve 94 s -> 8 s) and prove 19,908 of 84,731 sites.
Computed-key reads of a namespace still see every export.
