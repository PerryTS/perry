### Fix: the SHIPPED runtime was built with the wrong panic strategy (#7302)

The exception transport (#7305) requires the unwinder to step through runtime
Rust frames with `longjmp`-equivalent semantics, which is why
`[profile.release]` moved to `panic = "abort"`: under `panic = "unwind"`
rustc plants RFC-2945 abort-on-unwind guards in every `extern "C"` function
containing an interior Rust call, so a JS throw crossing such a helper — a
throwing getter, a `JSON.parse` error, a throwing `map` callback — aborts the
process instead of being caught.

`[profile.dist]` — what `release-packages.yml` builds the shipped
`libperry_{runtime,stdlib}.a` with — *inherits* `release` but then
re-declared `panic = "unwind"`, and an explicit re-declaration wins over
`inherits`. #7305 changed only `[profile.release]`, so **every local build,
every CI job and the entire parity suite were correct while the artifact
users install would have aborted on the first cross-helper throw.** Nothing
failed to compile and nothing went red; the two configurations differ only in
the profile the release workflow happens to use.

Confirmed directly from the rustc invocations (`cargo build --profile dist
-v`): before, `force-unwind-tables=yes` with no `-C panic=abort`; after, both
present. Caught before any release was cut from the merged EH work.

Guarded so it cannot recur silently: `crates/perry/src/panic_profile_contract.rs`
runs in `cargo-test` (per PR) and asserts that every profile which builds a
shipped runtime archive declares `panic = "abort"` — deliberately treating
`inherits` as *not* evidence, since an innocuous-looking override is exactly
the failure mode. A second test pins `-C force-unwind-tables=yes` in
`.cargo/config.toml`, because abort alone omits the tables and the transport
cannot step runtime frames without them. Both were falsified before landing:
re-introducing the `unwind` value fails the first test with the diagnostic
above.
