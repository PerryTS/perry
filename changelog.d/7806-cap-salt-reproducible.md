**Class-capture symbol names no longer depend on the working directory** (#7177) — the same source compiled from two different directories now produces byte-identical IR.

The per-module salt in `__perry_cap_<id>m<salt>` was an FNV hash of the **canonical absolute source path**, so it changed with the checkout location. Reproducible builds were impossible by construction, and every IR/object A/B harness had to independently discover the dependency and pin cwd — #7176's byte-neutrality harness measured **29 of 29 same-compiler control runs differing** once it used per-run `mkdtemp` directories.

Reproduced before touching anything: the same 8-line source compiled from two directories differed in exactly **4 lines of IR**, all of them the salt string (`m000059ddd802` vs `m0000f0a10f23`). Everything else — including the `prog_ts` module prefix on every other symbol — was already identical, which is the tell: the module NAME was reproducible all along, and only the salt reached past it to the filesystem.

So the salt now keys on the module name. That is not merely convenient, it is the identity that already satisfies all three properties the salt needs:

* **Reproducible** — it is what every other emitted symbol is prefixed with (`perry_fn_<mod>__…`, `perry_global_<mod>__…`), so it cannot vary with the checkout without breaking far more than this.
* **Unique per module** — it carries directory components, so `a/util.ts` and `b/util.ts` become `a_util_ts` and `b_util_ts`. Verified with two same-basename modules whose capture stashes must not merge: distinct salts, correct output.
* **Equal within a module** — same module ⇒ same salt, so same-module inheritance keeps sharing the parent's capture stash, which is the bug the salt was introduced for.

`with_class_id_start` keeps its old path-based behaviour for the `#[cfg(test)]` lowering entry points that have no module name; the production path uses the new `with_class_id_start_salted`.

Verified: IR byte-identical across two working directories (was: 4 lines differing); same-basename modules keep distinct salts and match Node; `test_gap_cap_salt_reproducible_7177.ts` passes byte-for-byte. The gap test deliberately covers the *properties* rather than the reproducibility itself — a `.ts` test cannot compile itself from two directories — pinning cross-module isolation, same-module inheritance sharing, and two distinct captures in one module, so a future re-keying that is too coarse or too fine fails rather than silently merging stashes. `cargo test -p perry-hir` 293 passed, `cargo test -p perry` 902 passed, `test_gap_class` 24/24.
