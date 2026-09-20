Gated `sep_jv` in `string/split.rs` behind `#[cfg(feature = "regex-engine")]`,
the only arm that reads it. Bound unconditionally it is an unused variable in
any build without that feature, so `RUSTFLAGS="-D warnings" cargo check -p perry
--bins` — one of the six compile commands `run_lint_gates.sh` derives — failed.

Worth recording why review missed it: a one-invocation whole-workspace build
**unifies cargo features**, so the regex engine is always on and the binding is
always read. Only the per-package `-p perry --bins` command, which does not get
that unification, sees it. This is the same trap as the `cfg(test)` one — the
narrower command is the one that tells the truth.
