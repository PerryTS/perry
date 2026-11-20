The Windows ARM64 release leg no longer requires the Android cross-compiled
archives it cannot build. The NDK ships no Windows-arm64 host toolchain, so
those libraries are staged into the bundle only on the x86_64 Windows runner
that can produce them.
