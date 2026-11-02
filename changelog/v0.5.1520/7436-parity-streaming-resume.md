### Parity runner: streaming progress, durable journal, and `--resume`

`run_parity_tests.sh` accumulated every result in a bash array and serialized it
once at the very end. A full sweep therefore produced no machine-readable output
for hours — a 4.5 h run was indistinguishable from a hang — and an interruption
at test 900 of 1178 threw away all 900 results.

One mechanism now supplies both properties: **every completed test is appended to
a JSONL journal the instant it finishes**, and that same append drives a stderr
progress stream.

- **Streaming progress.** Each result emits `[142/1178] (01:12:33) test_gap_foo … PASS`
  to **stderr**. stdout is untouched, because `run_module_parity.sh` scrapes the
  summary block out of a `2>&1` capture. The `[i/N]` denominator comes from a new
  selection pre-pass that resolves `--filter`/`--shard` before the run instead of
  `continue`-ing inside it; shard arithmetic and ordering are unchanged.
- **Durable journal.** Default `test-parity/reports/journal/parity_<selection>.jsonl`,
  keyed by suite/module/filter/shard so concurrent shards never share a file.
  Overridable with `--journal PATH` or `PERRY_PARITY_JOURNAL`. One
  open/append/close per line, so a `kill -9` loses at most the in-flight test. A
  journal write failure (a full disk being the realistic case) is now fatal
  rather than silently dropping results from the report.
- **`--resume`** replays the journal, skips what it already holds, and continues.
  It refuses to resume across a **different compiler/runtime build**: the journal
  header records a SHA-256 over `perry` *and* `libperry_runtime.a` /
  `libperry_stdlib.a`, plus the selection flags and host platform. mtime would
  not do — a binary swapped in place without a mtime change would compare equal.
  Archives are included because a stale `libperry_*.a` changes behavior while
  `perry` itself is untouched. The final summary states how many results came
  from the journal versus this run.
- **Pause.** SIGINT/SIGTERM finish or abandon the in-flight test cleanly, print
  the exact resume command, and exit 130/143. An interrupted run deliberately
  does **not** publish `latest.json` — that file feeds the gap gate, the
  threshold gate and the parity matrix, and a partial sweep consumed as a
  complete one would move a gate on tests that never ran. Cleanup now also stops
  the TLS companion server and reaps children scoped to the run's scratch dir.
- The final report is rebuilt **from the journal**, so a resumed run and an
  uninterrupted run produce identical JSON. Verified byte-for-byte against
  `origin/main` over the same filter, including the `[""]`-for-empty-failure-list
  quirk that `run_gap_tests.sh` and `parity_matrix_trend.py` filter with
  `select(. != "")`.

Two bugs found and fixed while testing this:

- A torn (newline-less) final line — what a `kill -9` mid-write leaves — caused
  the *next* appended record to concatenate onto it, so **both** became
  unparseable and a second, innocent test was silently lost. The journal is now
  sealed with a newline before appending on resume.
- `tests/test_parity_build_reuse.sh` asserted with bare `[[ ... ]]`. Under macOS's
  bash 3.2 a standalone failing `[[ ]]` does **not** trip `set -e` (bash 5 does),
  so every assertion in that file was inert on a Mac. They now fail explicitly;
  the conversion immediately surfaced a real header/grep mismatch.
