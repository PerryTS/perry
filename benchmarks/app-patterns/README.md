# App-pattern bench suite

Each kernel is a self-contained TypeScript file that exercises **one
pattern that real apps actually do** in a hot loop — not a microbenchmark
of a single language feature, and not a full app stack. The point is
to find where Perry actually loses to Bun on workloads that come up in
production code, not to measure isolated primitives.

## Running

```bash
# Build perry first
cargo build --release -p perry-runtime -p perry-stdlib -p perry

# Run all kernels
./benchmarks/app-patterns/run.sh

# Run a single kernel
./benchmarks/app-patterns/run.sh json_parse_1mb
```

Each run produces `results/matrix-<timestamp>.md` with a markdown
table comparing Perry / Bun / Node across all kernels. The **perry/bun
ratio** column is the priority signal:

- `< 1.0×` (✅ win): Perry is faster than Bun.
- `1.0–1.5×` (✓ ok): within margin.
- `1.5–2×` (⚠ borderline): worth investigating.
- `≥ 2×` (✗ slow): real gap, file a workstream.

## Current kernels

| File | Pattern | Why it matters |
|---|---|---|
| `json_parse_1mb.ts` | Parse 1 MB JSON × 30 | HTTP endpoints consuming upstream JSON |
| `json_stringify_1mb.ts` | Stringify 1 MB object × 30 | HTTP response builders, log emitters, queue producers |
| `string_concat_csv.ts` | Build 100k CSV rows | Log lines, exports, prompt templates |
| `string_template_interp.ts` | Template literal × 200k | Error messages, log formatting |
| `string_split_map_join.ts` | Split + filter + join × 50k | Line-oriented log/data parsers |
| `regex_replace.ts` | URL/email/number redaction × 1k | Markdown rendering, sanitization, log redaction |
| `map_1m.ts` | Map insert/lookup/iterate 500k | In-memory caches, ID lookups, request-id correlation |
| `promise_all_chains.ts` | 1k batches × 50 concurrent awaits | Per-request fan-out, batch processing |
| `object_deep_clone.ts` | Manual deep-clone × 50k | Immer-style copy-on-write, GraphQL resolver shaping |
| `date_format_parse.ts` | ISO format/parse × 100k | Log emission, audit timestamps |
| `buffer_transcode.ts` | utf8/base64/hex round-trip × 5k | Network frame parsing, encoding boundaries |

## Authoring rules for new kernels

- **One pattern per file.** Don't combine.
- **Run for 50–500 ms.** Long enough that hyperfine variance is small,
  short enough that the full sweep completes in minutes.
- **No external deps** (no network, no filesystem outside `/tmp`, no DB).
  Self-contained input. Defer DB/HTTP kernels until we have a real
  service-up bench harness.
- **Print exactly one `checksum:` line on stdout** with values that
  prove the work happened. The runner cross-validates Perry / Bun /
  Node checksums and warns on mismatch — a mismatch is a correctness
  bug, not a perf result.
- **Realistic shapes.** "Build a CSV row" not "concatenate 100k 'x's".
  The former is what apps do; the latter is a microbench.

## Categories not yet covered

These are deferred until there's a real reason to add them, or until a
specific gap surfaces:

- DB queries (in-process via `better-sqlite3` is doable; out-of-process
  via `pg` / `mysql2` / `mongodb` needs a service-up harness).
- HTTP server steady-state (Hono / Express dispatch, response build).
  Needs a long-running bench client; out of scope for the per-kernel
  hyperfine model.
- Crypto: bcrypt, JWT, AES (in-process via perry-ext-bcrypt /
  perry-ext-jsonwebtoken — could add but they're slow per op so the
  bench has to scale iterations carefully).
- File system / streams: mostly OS-bound, less pattern-specific.
- Worker threads / shared memory: not yet implemented in Perry.
- Cold start: runner shape is wrong for it (hyperfine warmup includes
  startup); needs `time perry script.ts` style harness.

## Extending

Drop a new `.ts` file in `kernels/`. Re-run `run.sh`. The runner
auto-discovers it.
