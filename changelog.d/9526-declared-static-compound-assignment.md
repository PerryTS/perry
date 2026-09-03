### Fixed

- Compound and computed assignments to declared static class fields now update
  the same backing cell used by direct reads. `K.n += 1`, `K["n"] += 1`, and
  `K[key] += 1` therefore persist in both strict modules and sloppy scripts,
  including when the class is reached through an alias. The runtime class-field
  table now remembers each declared static's GC-rooted LLVM global and mirrors
  accepted dynamic writes into it.
