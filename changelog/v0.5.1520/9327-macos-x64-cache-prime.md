The macOS x86_64 release leg can complete again. It must build on an Intel
runner to link an x86_64 LLVM, but a cold dependency build there exceeded
GitHub's six-hour job ceiling and was cancelled before its cache was saved —
so the leg could never warm the cache it needed. A preparatory job now compiles
the dependencies within the ceiling and saves them, leaving the release build
to compile only the workspace crates.
