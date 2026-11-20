**Manifest drift fix + gate relocation (#463/#512):** `API_MANIFEST` was missing 15 entries that
exist in `NATIVE_MODULE_TABLE` — 5 from `b36554a2d7` (node:http client `rawHeaders`/
`httpVersionMajor`/`httpVersionMinor`/`complete`, #10467/#10468/#10469) and 10 from `64ca0ebfe7`
(net.Socket surface cluster: `prependListener`/`prependOnceListener`/`pipe`/`unpipe`/`writable`/
`readable`/`writableEnded`/`readableEnded`/`_writableState`/`_readableState`,
#10441/#10442/#10444/#10465). Added to `crates/perry-api-manifest/src/entries/part_1.rs` and
`part_4.rs`, matching the file's existing convention of representing a zero-arg
`NativeMethodCall` property read as `ApiKind::Method { has_receiver: true, .. }`.

Nothing caught this drift when it landed because `every_dispatch_entry_has_manifest_counterpart`
lived in `crates/perry-codegen/tests/manifest_consistency.rs`, an integration suite CI's
`e2e-scoped` only runs per-PR when the diff names that file — the one file a PR that merely adds
`NATIVE_MODULE_TABLE` rows has no reason to touch. Moved the check to a `#[cfg(test)]` unit test
(`crates/perry-codegen/src/manifest_consistency.rs`) so it runs on every `cargo-test` invocation
regardless of diff scope; `perry-codegen` already depends on `perry-api-manifest` as an ordinary
dependency, so no new dependency edge was needed. The integration test's copy was removed (its
trigger condition was a strict subset of the unit test's); the file's other checks are unchanged.
