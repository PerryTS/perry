### Fixed

- `perry-ext-fetch` can build its own test binary again (#8155). `perry-runtime`
  is compiled here with `external-fetch-symbols`, so it declares `js_blob_new`,
  `js_file_new`, `js_headers_init_from_value` and `js_fetch_notify_signal_aborted`
  as `extern` and calls them, on the promise that the final link supplies them —
  which `perry-stdlib` does in a real binary but nothing does in this crate's
  test link. `cargo test --no-run` therefore failed to link, which kept the
  `ext-link` gate red on every PR; since `cargo-test`'s scope deliberately keeps
  `perry-ext-*` out of its fan-out (#7656), that left the whole ext family
  ungated, and this crate's own 13 tests had never run.

  The four symbols are now defined in a `#[cfg(test)]` module rather than for
  real: this crate is a `staticlib` whose objects win the final link ahead of
  perry-stdlib, so shipping them would silently replace perry-stdlib's
  Blob/File/Headers constructors and abort bridge everywhere. `cfg(test)` code
  never reaches `libperry_ext_fetch.a`, and perry-stdlib is absent from the test
  link, so no duplicate symbol can arise. A guard test asserts no shipped file
  in the crate defines any of the four, so moving them out from behind
  `cfg(test)` fails there rather than at a customer's link.
