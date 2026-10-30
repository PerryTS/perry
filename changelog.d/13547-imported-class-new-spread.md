`new C(...args)` on an imported binding collapsed the spread into a single
array argument, since the by-name path has no spread lowering. Imported
bindings now take the spread-aware construction path.
