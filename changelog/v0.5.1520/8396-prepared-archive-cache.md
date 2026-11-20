Closes #8396: incremental native builds now cache the prepared well-known
binding archives used by the linker. A one-file edit still relinks against its
new object, but it no longer repeats unchanged archive extraction, symbol-table
analysis, symbol localization, and rebuilding for every native shim.

The prepared-archive key fingerprints the exact runtime, stdlib, wrapper,
compiler, target/feature profile, and archive-tool bytes. Cached outputs carry
their own size and SHA-256 checks and are rejected if missing or corrupted.
Application objects and linker flags remain covered independently by the final
link cache, so changes to either still invoke the linker. Set
`PERRY_NO_ARCHIVE_CACHE=1` to bypass only the prepared-archive cache for
diagnosis.
