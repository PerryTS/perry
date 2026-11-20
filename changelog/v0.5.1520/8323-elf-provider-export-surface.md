### Fixed

- **The ELF provider-dylib fixture preserves rustc's export surface (#8160).**
  Its linker shim now passes rustc's generated version script through unchanged
  instead of replacing it with a script that made hundreds of Rust standard
  library, panic, and allocator internals globally preemptible. The gate also
  checks `js_gc_init` immediately after linking, so a future visibility
  regression fails with a direct diagnostic instead of surfacing much later as
  a provider/runtime identity mismatch.
