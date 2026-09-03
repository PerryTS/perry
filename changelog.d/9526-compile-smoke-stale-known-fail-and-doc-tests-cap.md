- **`compile-smoke`: pruned the two now-passing `KNOWN_FAIL` entries.** The job
  reported `1391 passed, 0 failed, 67 skipped` and still exited 1, because
  `test_issue_340_axios_response_props` and `test_issue_414_mysql_query_params`
  were fixed elsewhere and now compile — which is exactly what #9471's STALE
  guard exists to catch. The list is empty now; both expansions use
  `${KNOWN_FAIL[@]+"${KNOWN_FAIL[@]}"}` so an empty array is safe under `set -u`
  on bash < 4.4 (macOS ships 3.2, and this file documents caring about it).

- **`doc-tests`: `timeout-minutes` 120 → 240.** The cap's comment sized it
  against "macOS 34 min end-to-end", but the leg now runs the full xcompile
  matrix: it measured **119 min against the 120-min cap** (run 33598905771) and
  then overran it (run 33626738093), cancelled with its own doc-tests already
  reporting **30/30 passed**. A cancelled job fails `full-suite-gate`, so a
  minute of runner jitter cost a release cycle. 240 restores headroom while
  staying well under GitHub's 360-min hosted-runner ceiling.
