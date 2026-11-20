### Fixed

- **`lint` was red on `main`**, so every merge was bypassing a required context. `scripts/addr_class_inventory.py` flagged one violation — `crates/perry-runtime/src/object/tests.rs:646` `[gcheader-cast]`, a bare `(obj as *const u8).sub(GC_HEADER_SIZE) as *const GcHeader` added by #7928's inline-slot-floor probe.

  Replaced with the approved accessor `addr_class::try_read_gc_header`, which takes the object address and performs the header arithmetic behind its plausibility and slab checks. The audit passes again (1056 files, 546 ratcheted sites), with no new allowlist entry — the ratchet's whole point is that a fix deletes the violation rather than exempting it.

  Worth stating plainly because it is the shape CLAUDE.md warns about: a **required** context that is red trains everyone to bypass it, and a genuinely new violation would then land invisibly behind the old one.
