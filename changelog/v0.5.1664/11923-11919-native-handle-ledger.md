Add an exact, checked-in native-handle conversion ledger for #11919 L1/L3.
`scripts/native_handle_ledger.py` records 216 static/thread-local tables keyed
by native handle ids or GC-object addresses and 196 production handle-producer
sites, per file, and makes both populations monotone in the required `lint`
job. Its `--update` refuses increases, its pull-request merge-base comparison
prevents a diff from raising its own ledger, and its self-test plants table,
producer, comment-only, test-only, new-file, and stale-entry cases.

The L1 audit deliberately excludes maps keyed by strings/content, compiler
class ids, compiled code/literal/site metadata, GC page/card bookkeeping,
diagnostic stack hashes, the ext-http TLS configuration hash, and the internal
HTTPS-server port set: those numeric-looking keys are not native handle ids or
GC-object heap addresses. It includes vector/slab registries for Proxy,
EventEmitter, TUI, geisterhand, reclaimable or indexed common handles and
readline, plus the three handle-keyed maps hidden in ext-zlib's static state
bundle. Tests, `#[cfg(test)]`/`#[test]` items, and comments are never counted.
