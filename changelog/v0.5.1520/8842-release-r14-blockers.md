### Fixed

- Kept caught property-read `TypeError` throws unwind-capable across the runtime FFI boundary, made the regression test build the static runtime archive it actually links, and reconciled the Linux parity ratchet with the r14 release evidence.
