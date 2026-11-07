Route the util.parseArgs and util.styleText heap-type probes and the born-null
prototype test through the canonical address classifier, so a native handle in
the handle band is rejected before any GC header read. The instanceof
Function.prototype probe now tests for a reference rather than a raw heap tag.

better-sqlite3 `Statement#columns()`, `get()` and `all()` now root every
converted column value, row record and the result array across the
allocations that build them, so a collection triggered while a row is being
built can no longer leave a stale pointer in the result. The indexed Map
string-key match reads its payload through the string accessor.
