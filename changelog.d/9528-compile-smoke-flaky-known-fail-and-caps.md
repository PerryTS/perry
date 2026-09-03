- **`compile-smoke`: #9470's tokio-coherence pair is FLAKY, and the STALE guard
  is now advisory.** Run 33626738093 compiled all 1391 files clean; run
  33709074616 failed `test_issue_414_mysql_query_params` with
  "the wrapper archive(s) bundle a DIFFERENT tokio compilation than the stdlib
  archive" — with only workflow-file edits between the two trees. A STALE check
  is only sound for a *deterministic* failure: for a flaky one a single green
  run does not prove the entry is fixed. #9471 made it fatal, which cost a cycle
  in both directions — a lucky run tripped STALE, and pruning the entries then
  let the next unlucky run trip UNEXPECTED. The entries are restored and STALE
  now emits `::notice::`. The root cause is fixed on main by "isolate
  shared-tokio auto-opt graphs"; this release pin predates it.

- **`doc-tests`: `timeout-minutes` 120 → 240.** Its comment sized the cap against
  "macOS 34 min end-to-end", but the leg now runs the full xcompile matrix: 119
  min against the cap (run 33598905771), then an overrun (run 33626738093)
  cancelled with its own doc-tests already reporting 30/30 passed. On the raised
  cap it finished in **121 min** — one minute past the old limit.

- **`simctl-tests`: `timeout-minutes` 60 → 120.** Successful runs grew 42 → 45 →
  54 min; two consecutive runs on one commit then hit the cap at 61 and were
  cancelled (33709079451, 33718460967) before a third passed at 54
  (33727755737). A cancelled simctl run fails release-packages' exact-SHA gate,
  so that coin flip blocked releases outright.
