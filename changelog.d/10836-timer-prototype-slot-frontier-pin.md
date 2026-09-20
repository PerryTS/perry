`scripts/gc_runtime_root_holders.py` was red on this branch (independently of
the file-size and raw-handle work): the two `perry_thread_local!` slots backing
`Timeout.prototype` / `Immediate.prototype` are new rule-T holders that the
gate's call-graph walk does not reach.

They ARE visited — `scan_timer_prototype_roots_mut` walks both through
`RealmAtomicI64::with_slot` and is called from the registered
`object::scan_object_cache_roots_mut` — but the walk resolves symbols named in
function bodies and these slots are named only in a `static` initializer
(`RealmAtomicI64::new(&TIMEOUT_PROTOTYPE_SLOT)`), so the tool cannot see the
edge. This is the same shape, and the same limitation, as every
`*_ITERATOR_PROTOTYPE_PTR_SLOT` in `object/iterator_prototypes.rs`, which are
pinned on the frontier ratchet for exactly this reason.

Pinned the two slots the same way, each naming
`scanner: "scan_object_cache_roots_mut"` so the pin goes stale if that
registration is ever deleted. A pin is debt, not a GC-safety verdict; the
verdict is the scanner, which exists.
