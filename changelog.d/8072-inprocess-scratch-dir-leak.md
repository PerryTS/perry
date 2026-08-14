### Fixed

- **In-process LLVM compiles no longer leak an empty scratch directory per compile.**
  `compile_ll_inprocess_in`'s statepoint arm is the only in-process path that
  *creates* the per-compile scratch directory — it writes the assembly there
  before the compact-map rewrite — but its cleanup removed the two files it knew
  about and left the directory. One `perry_llvm_scratch_<pid>_<counter>` husk
  survived every compile.

  The clang path has always done `remove_dir_all`, under a comment saying "the
  directory cannot survive as an empty husk". Nothing went wrong while clang was
  the default; the leak began when the in-process backend became it, and the
  cleanup that only ever existed on the other path stopped running.

  This is what turned `repsel-census` red on `main`: the job's Temp-directory
  hygiene step reported `58 compile(s) left 58 entries in a temp directory that
  started empty`. The leak is counted in *compiles* rather than in distinct IR,
  which is what distinguishes it from the earlier `#7144` leak.

  The consequence was larger than a pile of empty directories. Temp-directory
  hygiene runs before `Promotion census` and `Sabotage check`, so both were
  skipped for every red run — the census gate itself had not executed on `main`
  since 2026-08-04. Because `repsel-census` is not a required status check, eight
  consecutive red nightlies blocked nothing and nobody was forced to notice.

  The regression test pins the statepoint lowering with `NativeRootsPin::native()`
  rather than trusting the host default. Only the statepoint backends route
  through assembly, and only that arm creates the directory, so without the pin
  the test passes on a host that never enters the arm — the vacuous shape this
  repo has been bitten by before. Verified by sabotage: restoring the two
  `remove_file` calls turns the test red.
