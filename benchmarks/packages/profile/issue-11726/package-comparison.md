# Package regression comparison

The core fix (`48f28ca8dc`) and baseline (`437520d02b`) were compiled independently with coherent, isolated runtime archives and automatic specialization disabled. All 51 native workloads compiled. At 0.1× the manifest iteration counts (warm-up unchanged), 50 rows pass the Node 26.5.1 stdout/checksum oracle in both arms. `mongodb/insert_find` retains the full-size run instead. The final IteratorValue getter-order follow-up (`ca77316359`) is covered separately by full-size MongoDB counters and profiles and the final-build regression fixtures; these package matrix binaries precede that follow-up.

`fastify/inject` fails identically in both arms with `Cannot read properties of undefined (reading 'once')`. An earlier full-size `dotenv/parse` run also has an unchanged Node/native checksum mismatch in both arms; its scaled run passes. These are pre-existing failures, not fixed by this PR. Shared-host results do not support wall-time claims.

| Workload | Instruction change | Peak RSS change |
| --- | ---: | ---: |
| `axios/get_json` | +2.73% | -0.10% |
| `axios/post_json` | -1.40% | -3.02% |
| `big.js/arith_chain` | +0.00% | +1.17% |
| `bignumber.js/arith_chain` | +0.20% | -0.08% |
| `commander/parse_argv` | +0.18% | -0.34% |
| `control/bare_loop` | +0.02% | -0.01% |
| `control/prop_read` | +0.01% | -0.67% |
| `cron/next_dates` | -0.08% | +0.22% |
| `date-fns/diff_interval` | +0.25% | +0.65% |
| `date-fns/format_add` | +0.92% | -0.57% |
| `dayjs/diff_startof` | +1.80% | -8.37% |
| `dayjs/parse_format` | -0.00% | -0.06% |
| `decimal.js/arith_chain` | +0.18% | -0.47% |
| `decimal.js/parse_sum` | +1.43% | -0.87% |
| `dotenv/parse` | +0.44% | -0.26% |
| `exponential-backoff/retry` | +0.19% | -0.82% |
| `fastify/inject` | Same existing failure | — |
| `fastify/listen_fetch` | -0.71% | -1.92% |
| `ioredis/pipeline` | -0.13% | +0.46% |
| `ioredis/set_get` | +0.11% | -0.48% |
| `jsonwebtoken/decode` | +0.06% | -0.22% |
| `jsonwebtoken/hs256` | +2.32% | +0.02% |
| `jsonwebtoken/rs256` | -0.00% | +0.72% |
| `lru-cache/churn` | -0.01% | -0.35% |
| `lru-cache/ttl_mixed` | +0.04% | +1.41% |
| `moment/diff_duration` | -0.26% | +0.69% |
| `moment/parse_format` | +1.04% | -0.77% |
| `mongodb/batch_query` | -9.37% | -11.55% |
| `mongodb/insert_find` | -82.01% | -79.42% |
| `mysql2/insert_batch` | +0.00% | +0.25% |
| `mysql2/select` | +0.06% | -3.19% |
| `nanoid/generate` | +0.51% | +0.09% |
| `node-cron/match` | +0.14% | -1.23% |
| `node-cron/validate_parse` | +1.02% | -0.70% |
| `node-forge/aes_cbc` | +0.17% | +0.07% |
| `node-forge/hmac` | +0.05% | -0.33% |
| `node-forge/rsa_sign` | -1.43% | -2.88% |
| `node-forge/sha256` | +0.45% | -1.21% |
| `pg/insert_batch` | +0.07% | +0.59% |
| `pg/select` | +0.04% | +0.42% |
| `qs/parse_nested` | +0.12% | -0.75% |
| `qs/stringify_nested` | +0.61% | -0.59% |
| `rate-limiter-flexible/consume` | +0.06% | +0.26% |
| `rate-limiter-flexible/get_penalty` | +0.08% | -0.12% |
| `redis/pipeline` | +0.50% | -0.16% |
| `redis/set_get` | +0.12% | +0.19% |
| `uuid/v4` | +0.48% | -0.01% |
| `uuid/v5_parse` | +0.21% | -0.52% |
| `uuid/v7` | +0.22% | +0.03% |
| `validator/batch` | -2.07% | +0.41% |
| `validator/sanitize` | -3.86% | -0.42% |

Repeated `axios/get_json`, `qs/parse_nested`, `node-forge/sha256`, and `validator/sanitize` with five RSS samples and three two-N instruction repetitions at the same iteration counts after noisy initial readings. The table uses those repeats; raw initial measurements and repeats are adjacent.
