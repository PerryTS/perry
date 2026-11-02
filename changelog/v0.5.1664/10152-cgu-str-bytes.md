### Fix split-unit dispatch descriptors referencing missing string bytes (#10152)

Close initializer dependencies for otherwise-unreferenced globals retained in
codegen unit 0. OpenTUI's node-backend chunk discarded a function after an export
getter claimed the same symbol, leaving its `segment` dispatch descriptor in
unit 0 while only the string initializer in unit 1 referenced the bytes.

Preserve existing ELF/COFF owners and add external declarations for these
dependencies; include the additional consumers in Mach-O's replication counts.
This keeps unrelated strings and single-unit cache globals in their existing
units and linkage. Regression coverage includes a reduced TypeScript module
compiled through both LLVM construction paths and unit tests for ELF, COFF,
Mach-O, and transitive initializer dependencies.
