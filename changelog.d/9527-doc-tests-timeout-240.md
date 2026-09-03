- **`doc-tests`: `timeout-minutes` 120 → 240.** The cap's own comment sized it
  against "macOS 34 min end-to-end", but that premise is stale — the macOS leg
  now runs the full xcompile matrix. It measured **119 min against the 120-min
  cap** (run 33598905771) and then overran it (run 33626738093), cancelled with
  its own doc-tests already reporting **30/30 passed**; the kill landed in the
  trailing cross-compile step. Since a cancelled job fails `full-suite-gate`, a
  minute of runner jitter cost a full release cycle. 240 restores headroom while
  still bounding a hang well under GitHub's 360-min hosted-runner ceiling.
