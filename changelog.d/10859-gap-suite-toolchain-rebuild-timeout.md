### Fixed

- **The gap suite reported a toolchain rebuild as a compile error.** Every
  fixture got one 300 s compile budget (#10757), sized on the belief that the
  fast-mode/`PERRY_SKIP_BUILD` tiers never pay an auto-optimize rebuild per
  test. They do: the #7629 block in `run_parity_tests.sh` unsets
  `PERRY_NO_AUTO_OPTIMIZE` for every fixture that routes a module to a
  `perry-ext-*` wrapper, so `perry` runs a `cargo build` of a feature-stripped
  runtime + stdlib + wrapper *inside* that budget, once per distinct feature
  set. Those rebuilds measure 270–300 s on GitHub's hosted runners, so a
  rotating handful of http/https/http2/net/ws fixtures was killed at exactly
  300.1 s and reported as `pass -> compile_fail` in run after run — 11 of them
  on #10918, 5 on #10892, 4 on #10930, all three PRs merged over the red.

  A compile that may rebuild the toolchain now gets its own budget,
  `PERRY_EXT_COMPILE_TIMEOUT` (default 900 s), selected by the property that
  predicts the cost — auto-optimize on *and* the fixture routes to an ext
  wrapper — not by test name. The ordinary 300 s budget is unchanged, so a
  genuine hang in a plain compile is still bounded at 300 s.

  A killed compile is also no longer printed as `(compile error)`. It says
  `compile TIMEOUT after <N>s — killed, not rejected`, and the persisted
  `*.compile_error.log` leads with the same line. The old message was
  indistinguishable from a compiler diagnostic and the real cause was only
  recoverable by subtracting two timestamps out of a CI log.
