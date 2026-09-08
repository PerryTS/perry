### Fixed

- Restore the scheduled native-roots gate on ARM Linux and macOS by handling
  libyaml's target-specific `c_char` signedness and retaining its configured
  LLVM 22 prefix for the in-process backend.
