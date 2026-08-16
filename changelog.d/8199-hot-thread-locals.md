### Fixed

- Convert four hot `thread_local!` declarations to `crate::perry_thread_local!`,
  which `self-test-checkers` was failing on across `main`
  (`node_module/source_map.rs`, `node_vm.rs`, `dyn_eval/interp.rs`,
  `module_require.rs`). All four are hot by #7469's own criterion: the first two
  hold GC roots that `scan_roots` / `scan_vm_roots_mut` walk on every
  collection, `interp.rs`'s is the AST-node → fn-id registry that re-evaluates
  per request, and `module_require.rs`'s is per-`require()` state. Converting
  rather than recording them cold is what the access pattern justifies.

  `interp.rs`'s second block stays raw on purpose — a one-shot `REGISTERED`
  flag, genuinely cold, and the one the allowlist already records for that file;
  converting it too would leave a stale entry, which the checker also rejects.
