Added opt-in execution of prebuilt Node-API v8 addons on desktop/server targets,
including exact package policy, authenticated relocatable sidecars,
`require()`/`process.dlopen()` loading, narrow host symbol exports, GC-safe
lifetimes, buffers, promises, async work, and threadsafe functions.
Also aligned Windows `@parcel/watcher` facade event paths with the published
`watcher.node` binding by hiding verbatim-path prefixes at the JavaScript boundary.
