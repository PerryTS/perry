`new EventEmitter()`'s construction memo now uses `perry_thread_local!`, so
reading it no longer costs a `_tlv_get_addr` call on Darwin.
`scripts/check_thread_locals.py` now resolves `#[path]` module attributes, so a
test-only file included under another name is no longer counted as shipping
code.
