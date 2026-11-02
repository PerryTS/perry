Built-in submodule singletons (`node:timers/promises`, `node:diagnostics_channel`,
`node:test`, ... export closures, namespace and default objects) live in one
slot record per submodule, indexed by the submodule's ordinal and the export's
ordinal, instead of std SipHash maps keyed by the addresses of the submodule and
export name strings. A lookup is two indexed loads; its cost no longer depends
on where the link put those strings.
