# R30 string flag investigation

**Not promoted or timed.** Concatenation now clears the JSON escape-free flag, and the callback/emitter tests pass. A new raw-surrogate diagnostic exposes a second invalid source for that flag.

Source `3c14d729947343431cc98e18fd78a3f06cd5f538` versus frozen R26 `3aac4d6335da54abeeed73df842decbbe6dd5d71` at workspace 0.5.1531. Neither is current-main measurement.

Not promoted or timed. 298 serial release JSON tests and 114 string tests pass (overlapping suites). All 81 original candidate checks and 20 expanded emitter executions pass; 26 matching IR files and six worker objects. Existing native root findings, lazy/getter/fraction limitations remain unsuppressed. A separate newly added raw-surrogate diagnostic fails complete Node output: R26 plain output fails on two rows; R30 adds six pretty/replacer mismatches. This invalidates the candidate despite its declared suite passing. The next revision validates non-ASCII borrowed strings before granting the escape-free flag.

The parser currently treats an unescaped JSON token as safe to copy directly into output. JavaScript input strings can contain raw lone surrogates; those still require JSON escaping. The R30 emitter extends an existing plain-stringify defect to pretty/replacer output. Full byte outputs, Node oracle, commands, and binary hashes are retained in the diagnostic archive.

The next source revision validates non-ASCII tokens and uses the existing WTF-8 builder for invalid UTF-8. Validated UTF-8 uses the existing vector UTF-16 counter. ASCII construction, GC policy, allocation thresholds and parse boundaries are unchanged.

The clean normal all-three-package production build completed in 346.629 seconds. Local lint passes 73/74 executed gates; public benchmark freshness fails, file cap passes. Compile-tier and CI-only lint gates were skipped. No remote stage, timing window or PR was created for R30.

| Artifact | R30 SHA-256 |
|---|---|
| perry | `b5f8496ed5211537d20d408982dd93c20f6ecbce74e14e87f0f4708bfb1be910` |
| libperry_runtime.a | `a2c70005b8136485623531660aeed2cf300c894530b94c845465ec1d14fd732a` |
| libperry_stdlib.a | `ef556d27b9bd9b0e70e3172f32240813a8dccd84cb181f8dd47e32bf64ebe0b0` |
