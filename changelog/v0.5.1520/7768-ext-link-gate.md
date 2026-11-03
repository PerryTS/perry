### Changed

- **CI: a per-PR gate now LINKS the `perry-ext-*` crates (#7656).** A `perry-runtime` change broke the link of five of them and no per-PR gate could have caught it (#7650, fixed in #7655) — it surfaced at the next tag, days later.

  `cargo-test`'s scope could not see it, and not by accident: `ci_test_scope.py`'s `_is_fanout_leaf` deliberately keeps `perry-ext-*` / `perry-stdlib` out of the reverse-dependency fan-out, because their unit tests are self-contained pure-Rust logic and re-running ~40 crates on every foundational change is the cost that scoping exists to avoid. That reasoning is right about the tests and silent about the **link**: these crates pull in a feature-stripped runtime through `perry-ffi`'s `runtime-link` built with `-Wl,-dead_strip`, so a new reference edge inside `perry-runtime` can keep alive a chain the stripper had been removing. In #7650 the edge was one added call (`pin_object` → `arena::classify_heap_space`) replacing a raw flag write, and the symptom was `Undefined symbols for architecture arm64`.

  So the new `ext-link` job builds and does not run: `cargo test --release --no-run` links the test binaries and stops. `--release` deliberately — `-dead_strip` is a release-profile behaviour, and a dev-profile build would be green through exactly this regression. The existing fan-out exclusion is left alone.

  **The package list is derived, not written down.** `scripts/ci_ext_link_scope.py` enumerates `crates/perry-ext-*` from the workspace, so a new ext crate is covered the day it lands — the failure mode #7748 had to repair in `ci_e2e_scope.py`, where a hand-maintained map named 3 of 24 suites and nothing could say so. Its self-test additionally asserts the five crates that actually failed in #7650 are still in the derived list, so a rename reports itself instead of quietly shrinking the gate.

  **The job asserts it linked something.** #7656's first requirement, and the failure mode this repo has shipped four times (#6942/#6946, #7024, #7025): a scope rule that selected zero packages would be green forever. The count comes from cargo's own `--message-format=json` artifact records, and zero is a hard failure with an error that says to fix the scope rather than trust the green.

  **Cost, measured, and why the scope is all 38 crates rather than #7650's five.** On an arm64 dev Mac: a COLD release target building only the five took **7:24** (7 test binaries); all **38** ext crates with `perry-runtime` already built took **4:10** (216 test binaries). The shared runtime build dominates, so widening the scope to every ext crate costs less than building the runtime once — the issue's "a wider set is better if it is affordable", answered with a number instead of an argument.

  Not made a required context yet, per CLAUDE.md's corollary — a new gate has never been green, so promoting it immediately blocks every open PR. Run it once, then promote.
