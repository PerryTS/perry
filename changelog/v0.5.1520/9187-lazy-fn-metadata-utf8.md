`js_register_function_name` and `js_register_function_source` validated UTF-8 at
registration. Module init calls them once per function in the bundle — 72,713
times for claude-code — and `core::str::from_utf8` was 2.17% of `cc --help`
doing exactly that. Function source text is registered for every function a
bundle *contains*, to serve `Function.prototype.toString()`, which most
programs never call.

Both registries now store raw bytes and decode on read. Nothing observable
changes: a name that is not valid UTF-8 was previously dropped at registration
and is now dropped at lookup, so both report "no name".
`PERRY_EAGER_FN_METADATA_UTF8=1` restores eager validation for A/B measurement.

One place needed the write side taught the same rule. An undecodable entry now
*occupies* the map where registration used to leave the slot empty, so
`register_function_name_if_absent`'s `or_insert_with` would have seen it as
present and let it shadow a valid name forever — the one case where "dropped at
lookup" would not have matched "dropped at registration". It now treats an
entry that fails to decode as absent, which is what `decode_registered` already
does on read.
