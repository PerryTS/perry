**The public-baseline freshness gate no longer fires on `Cargo.toml` changes that cannot move a measured number** (#7282, proposal 1 — the issue's "single biggest win").

`Cargo.toml` was fingerprinted whole-file. A new `perry-ext-*` workspace member, a dependency bump or a `[workspace]` restructure invalidated the published artifact without touching a benchmarked kernel — #6758/#6761's restructuring did exactly that, and the artifact then sat **40+ commits stale on a REQUIRED check**, so every later `lint` step (file-size, GC store-site inventory, addr-class audit, #7253's gate-wiring check) never executed in CI at all, and every merge needed an `--admin` bypass.

Only the `[profile.*]` tables now participate — `opt-level`, `lto`, `codegen-units`, `panic`, i.e. the things that genuinely change comparability. Extraction is textual and conservative (no TOML parser is guaranteed in the CI Python, and generator and checker must agree byte-for-byte); a section header ends the capture unless it is itself `[profile…`, so `[profile.release.package.x]` subtables are kept. Measured: 19,466 bytes → 8,681, all 45 profile tables retained, `[workspace.package]` and every dependency line gone.

**Sabotage-verified in both directions**, by exit code rather than by message:

| planted change | `ci_public_baseline_check.py` |
|---|--:|
| `[profile.release] opt-level = 3 → 2` | **exit 2** — "benchmark inputs changed; regenerate it" |
| new `crates/perry-ext-sabotage` workspace member | **exit 0** |

So the gate keeps blocking absolutely on what matters and stops firing on what does not — the issue's explicit requirement that it stay required and hard-failing.

**On the artifact's stored digests.** Narrowing the fingerprint changes both keys, so they are recomputed in the same commit. That is sound rather than an attestation: the artifact **matches under the broad fingerprint today** (verified before the change), and the narrow fingerprint covers a strict subset of those inputs — an artifact valid under the broad one is necessarily valid under the narrower one. No measurement was re-run and none needed to be: everything in the file except the two `freshness` digests is byte-identical, asserted programmatically. Independently, the `[profile.*]` extract is unchanged from the artifact's own commit (`38ff7eccc`) through HEAD, and across `HEAD~40` and `HEAD~120`.

Also worth recording: the issue's headline complaint — "`Cargo.toml` changes on every version bump" — was **already fixed** before this change. `_fingerprint_bytes` normalizes the `version = "…"` line to `0.0.0`, which is why the gate was green today despite `0.5.1355 → 0.5.1461`. This change addresses what was left.

Scope is proposal 1 only. `HARNESS_PATHS` still fingerprints scripts whose error handling cannot change a number (#7265's `run.sh` fix is the cited example); proposal 2's measurement-affecting/plumbing split is untouched. The deliberate circularity is preserved — `ci_public_baseline_check.py` stays out of `public_baseline.py` so the checker's own file is not in `HARNESS_PATHS`.

Verified: `tests/test_public_baseline.py` 7 passed, `benchmarks/ci_public_baseline_check.py` exit 0, file-size gate clean.
