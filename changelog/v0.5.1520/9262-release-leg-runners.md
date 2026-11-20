The macOS x86_64 and Windows ARM64 release build legs work again. The x86_64
macOS compiler was being built on `macos-15`, which is the Arm64 image, so it
linked an arm64-only LLVM and failed with undefined symbols once the in-process
LLVM backend became the default; it now builds on the Intel image. The Windows
Android cross-build step no longer runs on the ARM64 runner, which has no NDK
and for which the NDK ships no host toolchain.
