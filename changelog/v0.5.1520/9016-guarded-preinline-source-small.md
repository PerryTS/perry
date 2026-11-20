### Changed

- A guarded method specialization that is small at the source level but lowers to a large dispatch lattice is now admitted to the pre-statepoint inliner (up to 64 KiB of IR instead of 16 KiB), so one-statement leaves such as a sparse-set `add` flatten into their callers instead of staying a native call boundary.
