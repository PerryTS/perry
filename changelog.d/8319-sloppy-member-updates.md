### Fixed

- **Sloppy-mode property updates no longer throw when `[[Set]]` rejects the
  write.** Member `++`/`--` HIR now retains the source strictness and native
  codegen performs its write-back through strict-aware `PutValue` semantics.
  This makes updates of frozen or non-writable properties silently preserve
  their old value in sloppy scripts while keeping the required `TypeError` in
  strict code. `with` environment writes use the same semantics, fixing
  Test262 `built-ins/String/S15.5.5.1_A4_T1.js` from #5902.
