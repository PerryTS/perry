### Fixed

- **Successful in-process LLVM statepoint compiles no longer leave an empty
  scratch directory behind.** Cleanup now removes the per-compile directory as
  a unit, matching the clang path and preventing temporary-directory growth
  proportional to the number of compiles. A focused regression explicitly
  selects native-root lowering and verifies that repeated compiles leave no
  scratch entries.
