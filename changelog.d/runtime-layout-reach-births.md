### Fixed

- A program that births a runtime layout the compiler did not emit for it no
  longer aborts. Two causes: the eight node instance layouts (`Readable`,
  `Writable`, `Duplex`, `Transform`, `PassThrough`, `EventEmitter`,
  `LegacyStream`, `ServerResponse`) are consulted as one list, so each now
  declares every owner whose program probes it; and the probe itself
  (`instance_layout`, `derived_instance_layout`) treats a layout the program
  does not carry as "not a match" instead of aborting, through a new
  non-aborting `desc_if_defined()` accessor. A real birth still aborts, which
  is where the reach canary has something to catch.
- `fs.createReadStream` births `TIMEOUT` and `IMMEDIATE` while reading, so both
  layouts now list `node:fs.createReadStream` as an owner.

Before: `stream.Readable` and `fs.createReadStream` programs aborted at startup
with `runtime layout … is born, but the program's realm does not reach it`.
After: both run, and `cargo test -p perry --test cjs_wrap_builtin_require` goes
from 0 passed / 8 failed to 8 passed / 0 failed.
