A program that imports only `node:inspector` or `node:cluster` (for example
`new Session()` or `cluster.Worker`) no longer aborts with "native
constructor bag events.EventEmitter differs from its static layout". Those
modules now read `EventEmitter` through its own export, which installs its
statics (`once`, `on`, `listenerCount`, `defaultMaxListeners`, ...) before
the constructor is sealed on its static layout, the same way a direct
`node:events` import does.
