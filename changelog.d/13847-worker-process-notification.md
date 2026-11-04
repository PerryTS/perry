### Fixed

- Notify the creating process about new Workers through its public nextTick scheduler, retaining the returned Worker and honoring late process.emit lookup. Preserve the anonymous nonconstructible callback contract and declare the process realm dependency.
