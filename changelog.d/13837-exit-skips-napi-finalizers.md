`process.exit()` no longer tears the Node-API environment down. Like Node's
`reallyExit`, it runs the `exit` event and the `process.finalization` exit
callbacks and ends the process; environment cleanup hooks and thread-safe
function, async work and object finalizers run only at the natural end of the
event loop and when a worker exits. An addon whose thread-safe function
finalizer joins a thread that never ends no longer hangs `process.exit()`.
