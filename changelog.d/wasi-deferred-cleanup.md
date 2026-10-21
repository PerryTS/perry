Add deferred cleanup jobs for unreachable resource wrappers in native Perry
component hosts. Retain owners through live borrows; skip transferred tokens
and stale ownership generations; defer cleanup while the engine is active.
Preserve primary call failures and continue releasing other resources after a
destructor fails. Unreachable whole instances still free native engine memory
without invoking application callbacks from GC.

Root FinalizationRegistry construction across allocation of its entries array
and every pending cleanup job while delivery can allocate or invoke async hooks.
The native component-host ABI advances to 4 (command hosting uses ABI 3);
matching archives are required. Engine, FFI, moving-GC and cross-language
regressions are added. Execution qualification is pending. Automatic cleanup
inside Perry-authored WASI guests remains separate work.
