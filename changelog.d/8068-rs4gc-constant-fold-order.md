### Fixed

- Canonicalize construction-time constant folds before RS4GC so textual and native in-process LLVM construction emit identical machine code and compact GC maps while retaining live dynamic roots (#8065).
