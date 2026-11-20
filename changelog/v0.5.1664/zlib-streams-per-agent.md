zlib streams now belong to the thread that made them. Their state and queued
`data`/`end` events were process-wide, so with a `worker_threads` worker using
`createGunzip()` (or any zlib stream) the main thread could take the worker's
events and call its listeners on the wrong heap, and the worker's stream came
back short or never ended. Each agent now keeps its own streams and events, a
worker delivers its own between messages, and each thread roots its own
listener closures.

A `worker_threads` worker also gets its parent's messages from its own event
loop now, so a worker blocked in a nested wait (Perry drains
`Readable.from(asyncIterable)` that way) still receives the message it waits
for, instead of hanging or ending the stream early.
