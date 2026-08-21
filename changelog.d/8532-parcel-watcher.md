feat(runtime): add a native `@parcel/watcher` 2.5.x compatibility facade backed
by OS filesystem events. OpenCode's platform-selected watcher packages now route
to one GC-rooted native implementation with coalescing, rename, ignore,
snapshot, overflow-rescan, and safe-unsubscribe semantics instead of falling
back to a no-op when its Node-API addon cannot load. A 20,000-file idle tree
used about 1.3 ms of process CPU over five seconds on macOS.
