Fix workers that use the program's own entry module. Worker programs now give
that module the same guarded initializer as imported modules, so its body,
string pool, globals and static imports initialize once on each thread. The
process entry keeps startup and its event loop and calls that initializer.

Register the already compiled program entry in the existing worker entry table
and accept URL literals naming it. Include runtime and CommonJS namespace
constructors in the per-thread module-state decision and prepare a CommonJS
entry record on each worker thread. This also supports `new Worker(__filename)`,
`new Worker(fileURLToPath(import.meta.url))`, runtime filename dispatch,
`workerData` and top-level await in a self-spawning worker.

Add Node-parity regressions for these forms, independent entry/imported module
state across two workers, and a timer-backed top-level await in the worker.
