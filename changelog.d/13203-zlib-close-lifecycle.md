Fix `node:zlib` stream close state so completed destruction is visible before queued error and close events, while deferred custom destruction remains pending until its callback.

Restore the inherited `_closed` getter using codec-handle truthiness independently of Transform destruction.

Expose genuine codec-handle `close()` to release the native codec idempotently without destroying its Transform.
