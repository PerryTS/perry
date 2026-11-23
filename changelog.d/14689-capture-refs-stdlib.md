The stdlib's closures (Web Streams, `events.once` / `events.on`, the sqlite transaction
wrappers) store a captured promise, closure or state array as a tagged pointer value
(`closure_set_capture_ref` / `closure_capture_ref`) instead of a bare heap address.
No behaviour change.
