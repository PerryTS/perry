Add the manually dispatched `wasi-hosts` CI suite to qualify native P1 and typed
P2 hosting on Linux x86_64, macOS ARM64 and Windows x86_64. Build matching compiler,
engine, runtime and instrumented archives, execute both host crate unit suites,
compare the complete P1 suite against the pinned Node oracle, and run the native
P2 component acceptance suite.

Qualification requires the expected exit statuses, matching P1 output, actual
moving collections, evacuation checks and quarantine witnesses. Record source
and artifact identities alongside retained logs. The suite summary rejects
skipped, missing, failed and cancelled platform jobs. This starts as a manual
qualification suite; platform support is not claimed before execution evidence,
and existing required gates are unchanged.

Use an explicitly opened descriptor in the P1 stdio-constructor fixture. A
hard-coded descriptor 3 can name Node's internal event-loop handle but be absent
in Perry, making that comparison depend on process internals. All validation
cases remain. Initialize Windows C++ tools through Developer PowerShell because
the vendored CommonJS action is incompatible with the repository's ESM scope.
