# Workload 4 — Fastify HTTP throughput

Long-running Fastify HTTP server driven by `oha -c 10 -z 15s` per
measured run. Captures req/s, p50/p95/p99 latency, peak RSS.

## Kernels

| Kernel | Route | Response | Exercises |
|---|---|---|---|
| `minimal` | `GET /` | `{ok: true}` JSON | Framework overhead + JSON.stringify of one-field object |
| `text` | `GET /` | `'pong'` text/plain | Primitive response fast path — no JSON.stringify, no per-response allocation |
| `parametric` | `GET /users/:id` | `{id: <param>}` JSON | Router pattern match + per-request params object |

Each kernel ships in two languages: `perry/` (compiles to native binary
via `perry compile`) and `node/` (run via `node --import tsx`,
upstream `fastify@^5.2.0` from npm).

## Running

Default sweep excludes workload 4 (it requires `oha` and a network
loopback port; opt-in to keep CI deterministic):

```bash
HONEST_BENCH_ONLY=4 ./run.sh
```

Honoured env vars (overrides the defaults inside
`harness/run_http_bench.sh`):

- `HONEST_BENCH_WARMUP=1` — number of oha warmup sweeps (each
  `HONEST_BENCH_HTTP_WARMUP_DUR` long, default 5s)
- `HONEST_BENCH_MEASURED=5` — measured runs per kernel/language pair
- `HONEST_BENCH_HTTP_DURATION=15s` — per-measured-run oha load duration
- `HONEST_BENCH_HTTP_CONC=10` — oha concurrent connections (a small
  concurrency appropriate for a per-request-latency-bound workload)
- `HONEST_BENCH_HTTP_PORT=18080` — port the kernel binds
- `HONEST_BENCH_OHA=/tmp/oha` — path to the oha binary

## Result shape

Each row in `results/results.json` has the standard honest_bench fields
(`workload`, `language`, `binary`, `run`, `wall_ms`, `max_rss_kb`,
`exit_code`, `stdout_first/_last`, `output_match`) plus four
HTTP-specific keys:

- `rps` — requests / second (higher is better)
- `p50_ms`, `p95_ms`, `p99_ms` — latency percentiles in milliseconds
- `success_rate` — fraction of 2xx/3xx responses (0..1)
- `status_codes` — full HTTP status distribution

`wall_ms` is synthesised as `1_000_000 / rps` so the existing
`scripts/summary.py` "lower is better" sort still ranks correctly.

## Why no Bun reference output

Output-correctness checks (`harness/check_output.py`) are skipped for
workload 4. There's no canonical response payload to sha256 — every
response is `200 OK` with a small known body, and the metric of
interest is rate, not content. `output_match` is reported as `null`.

## Acceptance bar (relative to the Fastify perf work)

This workload backs the per-PR verification of the Fastify perf sweep.
It pins a fixed client load (`oha -c 10`) against each kernel so a PR's
mean `rps` can be compared against the prior PR's run on the same
hardware. The intent: as the Perry Fastify serving path lands its perf
fixes, each subsequent PR's mean `rps` must beat the prior PR's run on
at least one kernel without regressing the others. The Node kernels run
the same routes on upstream `fastify` as a fixed external reference
point.
