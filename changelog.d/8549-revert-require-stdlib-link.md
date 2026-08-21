**Revert "treat `require(<builtin>)` as a stdlib requirement" (#8548) — it caused a runtime-only link failure.**

#8548 fixed a real bug (#8547: `require('http')` linked runtime-only, so every stdlib-backed builtin reached that way returned `undefined`, and `issue_4903_listen_callback_deferred` went 0/2 → 2/2). It also broke `issue_5247_property_read_source_location`, which had been passing:

```
compile ["--debug-symbols"] must succeed; stderr:
/usr/bin/ld: libperry_runtime.a(...): in function `js_webassembly_validate':
  undefined reference to `perry_wasm_host_validate'
  undefined reference to `perry_wasm_host_module_new'
  … collect2: error: ld returned 1 exit status
```

**Attribution is solid, which is why this is a revert and not a follow-up patch.** The suite passed on `1c2326554` (the commit immediately before) and fails on `b00e261f8` (with #8548) in **both** sweep runs of that SHA — deterministic, not a flake. It does *not* reproduce locally, which is the tell for a feature-unification effect rather than a logic error: under `cargo test --workspace` cargo unifies features across the whole graph, so pulling perry-stdlib onto more compile paths changes what `target/debug/libperry_runtime.a` contains. The archive then carries `webassembly.rs`'s code, whose `extern` declarations are satisfied only by the separate `perry-wasm-host` crate — which a runtime-only link line does not include.

So the honest position is that #8548's *diagnosis* stands and its *link-mode consequence* was not thought through: making a program need the stdlib is not free, and the runtime-only link path is not prepared for everything the stdlib's feature set drags into the runtime archive. Fixing #8547 properly needs that interaction handled — either by ensuring `perry-wasm-host` is on the link line whenever the runtime archive contains wasm code, or by keeping the wasm surface out of it.

Net effect of this revert: `issue_4903` returns to failing (2 tests, the pre-existing #8547 symptom) and `issue_5247` returns to passing (3 tests). Main is red either way; this restores the *known* red rather than a new link-level one, which is the safer state to leave a release branch in — a link regression can bite real builds, not just tests.

Reopens #8547.
