Remove the transitive Tokio dependency introduced by wasm-bindgen-futures
0.4.79 through printpdf's browser target. Pin that bridge to its last Tokio-free
release, 0.4.78, and resolve the matching wasm-bindgen packages together.

The dependency ban now checks every repository Cargo manifest and lockfile,
including nested fixtures, inactive target configurations, renamed dependencies
and future tokio-* packages. Offline checks run before Cargo, and full metadata
checks use --locked. An independent CI preflight runs on every PR and core run;
the final gate requires explicit success, including on docs-only PRs. Existing
Tokio-free WASI component work remains on its separate branch.
