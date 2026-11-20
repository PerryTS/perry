Fix `perry run --local` rejecting prebuilt installations whose cross-compiled
runtime libraries are bundled next to the executable. Local/remote selection
now uses the compiler's library search, including target-specific install
directories, compressed archives, Apple platform suffixes, and
`PERRY_RUNTIME_DIR` / `PERRY_LIB_DIR` overrides.
