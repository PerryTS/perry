### Fixed

**`perry-codegen` integration suites can declare which root lowering they assert (#7493).**

#7370 made native roots (RS4GC statepoints) the default lowering, and `NativeRootsPin` was added so a test asserting on shadow-stack IR could say so. The in-crate unit tests were repaired with it; the five integration suites that assert the same mechanics were not, because `NativeRootsPin` is `#[cfg(test)]` and `tests/*.rs` link the crate as an ordinary external consumer — the item does not exist for them. Those suites run nightly/at-tag only, so nothing turned red at merge time and `shadow_slot_hygiene` sat at 0/12 on `main`.

The pin is now reachable as `perry_codegen::testing::NativeRootsPin`, behind a `testing` cargo feature that only `perry-codegen`'s own `[dev-dependencies]` entry on itself enables. It is a feature rather than a `#[doc(hidden)] pub` because with the feature off the pin, its thread-local and the branch it adds to `rs4gc_enabled()` are `#[cfg]`-ed out of the artifact — absent, not private — and cargo resolves dev-dependency features only for test/bench targets, so no production profile can reach it (`cargo tree -e features,no-dev` shows `default` + `llvm-inprocess` only). `NativeRootsPin::native()` joins `shadow()`: a pin also outranks `PERRY_RS4GC`, which is what keeps a pinned assertion meaning the same thing during a `PERRY_RS4GC=0` sweep.

Pins are per test, not per file — the files disagree internally:

| suite | shadow | native | unpinned | before → after |
|---|---|---|---|---|
| `shadow_slot_hygiene` | 12 | – | – | 0/12 → 11/12 |
| `scalar_replaced_slot_roots` | 11 | – | – | 2/11 → 5/11 |
| `temp_root_operand_temporaries` | 2 | – | 17 | 12/19 → 13/19 |
| `temp_root_argument_temporaries` | – | – | 7 | 3/7 → 3/7 |
| `native_proof_regressions` | 2 | 15 | 238 | 249/253 → 253/255 |
| `native_proof_buffer_views` | – | 1 | 31 | 28/30 → 30/32 |

Three tests were pinned although they were **passing**: `numeric_only_scalar_replaced_{object,array}_emits_no_rooting` and `a_collection_free_construction_emits_no_this_slot_root` assert `bind_calls(&ir) == 0` / `!contains("@js_shadow_slot_bind")`, which under the native default is true of every program. They were green without their subject running. Two of the three now fail for a real reason (#7504) — a red test that measures something beats a green one that does not.

The fifteen `invalidation` pins are `native()` for a different reason: `assert_buffer_store_uses_dynamic_fallback` proves the absence of a native buffer GEP with a module-wide `!ir.contains("getelementptr inbounds i8")`, and the shadow lowering's inline slot addressing emits that instruction for unrelated reasons, so `PERRY_RS4GC=0` made them report a stale proof that was never there (#7505).

**A poisoned `ARTIFACT_ENV_LOCK` turned 4 failures into 55.** `native_proof_regressions` reported 55 failures at default parallelism and 4 under `--test-threads=1`; 51 of the 55 were `PoisonError` — #7490's shape again. `PERRY_NATIVE_REPS*` are process-global and the restore was hand-written after the compile, so a panic inside `compile_module` left them installed and every later unlocked compile wrote artifact JSON into a directory another test was reading; the torn read panicked inside the lock. Both suites' copy-pasted harnesses are replaced by one `tests/native_proof_support/mod.rs` with a poison-tolerant accessor, an RAII env guard and an artifact reader that treats a foreign or half-written neighbour as noise. Two sabotage tests plant each failure shape and fail against the pre-fix code. Default-parallelism result: 198/253 → 253/255.

**Tripwire, in the required tier.** `codegen::testing_feature_gate_tests::host_target_lowering_default_is_native_roots` lives in `src/`, so it runs in `cargo-test`: a future flip of the lowering default fails there, in the PR that makes it, naming the suites that then need re-pinning. It asserts its subject is live (both pins must give different answers; the `arm64_32-apple-watchos` arm must give the opposite default), so a constant-folded `rs4gc_enabled()` fails it rather than passing it. A sibling gate scans every workspace manifest and fails if a non-dev dependency edge ever enables the `testing` feature.

Fifteen failures remain across these suites, none of them #7370's and none hidden: #7494 (three, pre-existing), #7503 (ten — the temp-root suites assert the pre-#7487 FFI spelling, and the eight that still pass are vacuous), #7504 (six plus `flat_const_row_aliases`), #7506 (one). No test was deleted, skipped or weakened. The per-PR source→suite mapping for these six suites is designed in #7507 and deliberately **not** landed here: they are not green, and a non-required job that is red on most PRs can never be promoted. The coverage question the pins raise — nine root-lowering mechanics with no assertion against the lowering that actually ships — is #7502.
