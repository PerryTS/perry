### Fixed

- **`Warnings (product)` and `Warnings (host-compatible)` are green again.** Both
  legs of the `rustc-warnings` gate ran with `RUSTFLAGS: -D warnings` and failed
  on `main` and every PR with `error: unused doc comment`.

  A `///` block sat directly above a `crate::perry_thread_local!` invocation in
  `gc/roots/stack_maps.rs`. rustdoc discards a doc comment on a macro
  invocation, so rustc warns — harmless as a warning everywhere else, fatal
  under `-D warnings`. It arrived with #8084's #7803 native-slot verifier.

  The text is worth keeping, so it becomes a plain `//` comment rather than
  being deleted, with a note saying why it cannot be `///`.
