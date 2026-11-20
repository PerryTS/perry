Remove redundant local copies and unread inert assignments from eligible
straight-line synchronous functions before codegen emits string-sharing and
root-shading work. Global and static-field stores now omit incremental root
shading for values proven scalar by construction; unknown and heap values
retain their barriers.

On the ARM64 instruction probes at the default LLVM `-Os` level, three local
aliases drop from 43 to 3 instructions, an alias live across a call from 70 to
54, and a constant global write from 11 to 6. These are static code counts,
not throughput measurements. Adds unit tests, Node-parity and moving-GC
coverage, and a reproducible instruction census.
