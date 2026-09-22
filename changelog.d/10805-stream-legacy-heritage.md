### Fixed

- **`class X extends Stream` (the bare `node:stream` base) — `pipe()`, the
  EventEmitter listener/emit surface, and `instanceof Stream` are no longer
  missing.** `#10649` fixed the analogous dynamic-heritage dispatch for
  `Readable`/`Writable`/`Duplex`/`Transform` but stopped short of `Stream` —
  the base those four derive from. Every heritage shape (bare import,
  namespace member, or CJS destructured `require('stream')`) now installs
  the correct surface, matching Node byte-for-byte. `instanceof Stream` is
  scoped to genuine `extends Stream` subclasses only — a plain
  `extends EventEmitter` class does not newly satisfy it. `PassThrough`
  (`#10745`) is a separate, deeper gap and remains unaffected.
