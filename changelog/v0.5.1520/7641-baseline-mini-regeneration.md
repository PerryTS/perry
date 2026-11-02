### Benchmarks

- **The public Node/Bun baseline is regenerated on the mac mini (#7641).**
  `lint`'s freshness gate had been red since #7601 edited
  `benchmarks/honest_bench/workloads/1_json_pipeline/perry/json_pipeline.ts`, a
  fingerprinted harness file. `public_baseline.py check` exits 0 again.

  **Host change, stated rather than implied**: `host.cpu` moves
  **Apple M1 Max / 64 GB → Apple M1 / 8 GB**, because the dev Mac cannot reach
  the harness's CPU-quiet gate. This **steps the published series** — rows are
  not comparable to the 2026-08-07 artifact. The relative Perry/node/bun
  ordering carries over; the absolute numbers are a slower, smaller-memory
  machine, and the generated table records the host so a reader can see it.

  Three mechanical failures preceded the successful run, each recorded because
  the next regeneration will meet them:

  1. **Three undeclared harness deps were missing on the mini** — `hyperfine`,
     `gtimeout`, `esbuild`. The harness version-gates node and bun but simply
     dies without these.
  2. **The harness refuses to measure a dirty tree**, and a prior aborted run's
     generated `RESULTS.md` files are exactly what dirties it.
  3. **Hyperfine exports go to fixed `/tmp` paths and the write does not
     truncate.** Stale exports from an earlier session left a trailing
     document, so a shorter new write produced two JSON documents in one file
     and the reader died with `JSONDecodeError: Extra data`.

  (2) and (3) are handled in the launcher, **deliberately not in `run.sh`** —
  that file is in `HARNESS_PATHS`, so editing it would invalidate the artifact
  the run exists to produce.

  Also corrects a stale note: the zig-link failure previously recorded for the
  mini (`__availability_version_check`) no longer reproduces — both
  honest_bench zig workloads build there with the pinned 0.15.2. The mini is a
  viable baseline host provided the five undeclared deps are present.
