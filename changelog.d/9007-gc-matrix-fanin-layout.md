The `gc-stress` fan-in now downloads the GC representation-matrix shards with
`merge-multiple: true` and reads them from a flat directory.

`actions/download-artifact@v8` extracted a single matching artifact directly
into `gc-repsel-shards/` rather than into a per-artifact subdirectory, so the
nested glob `gc-repsel-shards/gc-repsel-matrix-shard-*/gc-repsel-matrix-*.json`
matched nothing and the step died on the literal pattern under `set -euo
pipefail`. #9004's run 33228890420 produced a complete one-shard matrix
(`PASS=436 FAIL=0`) and still failed the fan-in.

Flattening makes the one-shard PR tier and the multi-shard Full tier use the
same layout. Shards write distinct filenames (`gc-repsel-matrix-$SHARD.json`),
so merging into one directory cannot collide, and the merge itself is unchanged:
`--expect` still requires exactly the planned number of reports, rejects a
duplicated or missing shard index, and verifies each test belongs to its
deterministic shard — a flattened layout cannot quietly reduce coverage.
