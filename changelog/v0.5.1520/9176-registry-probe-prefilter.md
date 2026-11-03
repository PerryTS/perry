`is_registered_buffer` and `is_uint8array_buffer` sit on generic paths — every
untyped element access reaches the second one — and both had an idle latch that
stops helping the moment a program registers its first buffer. After that, all
~216 probe sites paid a TLS access, a `RefCell` borrow and a hash lookup to ask
whether an arbitrary pointer was a buffer, and almost every caller was asking
about something that was not. Together they were 5.3% of `cc --help`.

Each thread-local registry now tracks the smallest and largest address ever
inserted, as a conservative filter in front of the hash. The range only widens
and every registration extends it before inserting, so an address outside it
cannot be in the set: rejecting is sound and accepting falls through to the
lookup that was already there. `PERRY_BUFFER_RANGE_FILTER=0` restores the
unconditional lookup.

`is_uint8array_buffer_slow` also took the process-global mutex on every
thread-local miss — the one probe in this family whose miss cost a lock rather
than a hash. It now consults that registry only behind a
`EXTERNAL_UINT8ARRAYS_NONEMPTY` latch, matching the gate
`is_registered_buffer_slow` already had.

That latch made the two inserters into the global registry a matched pair, and
`js_buffer_mark_as_crypto_key_external` was not arming it — so a WebCrypto key
registered on one thread became invisible to `is_uint8array_buffer` on every
other thread: the address was in the registry and the probe answered "no". The
registry is process-global precisely to be visible across threads, and the
thread-local set that masks the bug on the registering thread covers no other.
Both inserters now go through one helper that arms before inserting, and
`buffer/header_latch_tests.rs` asks from a fresh thread, which is the only
vantage point where the thread-local set cannot answer first.
