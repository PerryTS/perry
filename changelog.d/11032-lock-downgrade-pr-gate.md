### Fixed

Pull request CI now rejects `Cargo.lock` changes that lower a dependency for a
shared consumer, including `tempfile` resolving `getrandom` from 0.4.2 back to
0.3.4, before the merge can silently undo a newer pin.
