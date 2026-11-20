### Fixed
- Include raw `thread_local!` declarations in the GC holder inventory, so skipping Perry's TLS convention cannot hide a new opaque holder. Existing uncovered declarations join the identity ratchet as explicit audit debt.
- Give the GC census's deliberately untraced address snapshot a source-pinned, non-moving collection-window contract, and move its production TLS into the hot TLS registry.
