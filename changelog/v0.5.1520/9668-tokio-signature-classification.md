- **`compile-smoke` classifies the #9470 tokio flake by ERROR SIGNATURE, not by
  test name.** The flake is a property of the auto-optimize build, not of any
  particular test — it lands on whichever tokio-using wrapper the run routes
  through — so a name list is always one test behind. That cost three cycles to
  learn: `KNOWN_FAIL` held `test_issue_340_axios_response_props` and
  `test_issue_414_mysql_query_params`, and run 34355138005 then failed on
  **`test_issue_9310_mysql2_param_values`** with the identical error while both
  listed entries passed.

  A failure is now tolerated when its `*.compile_error.log` contains
  `bundle a DIFFERENT tokio compilation`. That string comes from perry's own
  linker refusing the link
  (`crates/perry/src/commands/compile/shared_tokio.rs`), so it cannot be confused
  with a genuine compile error.

  Verified it does not blind the gate: a fabricated `error[E0308]` still fails,
  and a failure with **no** log also fails — an unexplained failure is never
  assumed benign. The root cause is fixed on main by "isolate shared-tokio
  auto-opt graphs"; this pin predates it.

  This is the second time in this campaign that matching by name rather than by
  cause produced a one-item-short list (the other being the apt pin's package
  glob, which missed `libllvm22` and `libclang1-22`).
