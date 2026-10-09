### Fixes

- Preserve the numeric encoding of registered Web Streams receivers when rooting native method calls. A stream id encoded as a heap pointer could crash precise root scanning during an incremental collection.
