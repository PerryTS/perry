### Add native Windows ARM64 builds and packages

Perry now supports `--target windows-aarch64` (with `windows-arm64` as an
alias), selects matching MSVC and Windows SDK tools and libraries, and ships a
native `@perryts/perry-win32-arm64` package and GitHub release archive. The
plain `windows` target follows the host architecture on Windows while keeping
its existing x64 behavior when invoked from another operating system.

The Windows runtime also uses the ARM64 VCRuntime `setjmp` implementation, and
a native Windows ARM runner now builds Perry and executes a generated ARM64
smoke binary in pull-request CI.
