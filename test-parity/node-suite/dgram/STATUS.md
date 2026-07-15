# `node:dgram` granular parity status

## Upstream evidence

This expansion was assessed against primary repositories captured on 2026-07-15:

- Node.js [`34c28d5a`](https://github.com/nodejs/node/tree/34c28d5a69f4f00cd599adcbe57834435d3a683b/test): 82 `test-dgram*` files in `test/parallel` and `test/sequential`.
- Deno [`34f9f47f`](https://github.com/denoland/deno/blob/34f9f47f42d2a316efb8245aadc343f8d7cdf5c4/tests/node_compat/config.jsonc): 69 selected `parallel/test-dgram*` or `sequential/test-dgram*` entries.
- Bun [`da08a6b8`](https://github.com/oven-sh/bun/tree/da08a6b8da3fdde3da8aa7e1453584aa681e9c04/test/js/node): 68 copied upstream `test-dgram*` files plus Bun's focused `node-dgram.test.js`.

The Perry fixtures are diagnostic adaptations, not verbatim copies. They use ephemeral ports, loopback addresses, sequential round trips, and deterministic summaries rather than upstream harness helpers or fixed ports.

| Perry area | Representative Node files | Deno selection | Bun copy |
| --- | --- | --- | --- |
| socket and send validation | `test-dgram-createSocket-type.js`, `test-dgram-send-bad-arguments.js`, `test-dgram-send-invalid-msg-type.js` | all 3 | send cases (2/3) |
| bind and close lifecycle | `test-dgram-bind.js`, `test-dgram-bind-default-address.js` | both | both |
| AbortSignal | `test-dgram-close-signal.js`, `test-dgram-abort-closed.js` | both | both |
| connection state | `test-dgram-connect.js` | selected | copied |
| send overloads and byte counts | `test-dgram-send-callback-buffer.js`, `test-dgram-bytes-length.js`, `test-dgram-connect-send-callback-buffer.js` | all 3 | all 3 |
| omitted host | `test-dgram-send-default-host.js`, `test-dgram-connect-send-default-host.js` | both | both |
| empty and multiple sends | `test-dgram-send-empty-buffer.js`, `test-dgram-implicit-bind.js` | both (empty buffer is Darwin-disabled/flaky) | both |
| callback timing | `test-dgram-send-callback-recursive.js` | selected | copied |
| queue and reference state | `test-dgram-send-queue-info.js`, `test-dgram-ref.js`, `test-dgram-unref.js` | ref/unref (2/3) | ref/unref (2/3) |

## Current coverage

The directory contains 15 fixtures: the original 4 broad cases and 11 granular cases added in this expansion.

- `validation/`: socket-type matrices, message/list validation, and port errors.
- `lifecycle/`: default/port/options bind overloads with deterministic close, plus AbortSignal validation and post-close abort behavior.
- `connection/`: invalid ports, pending/connected state guards, disconnect errors, and reconnect state.
- `send/`: unconnected and connected string/typed-array overloads, omitted-host behavior, empty datagrams, multiple implicit-bind sends, callback byte counts, and callback asynchrony.
- `metrics/`: queue metrics and `ref()`/`unref()` identity before bind, after bind, and after close.
- Existing broad cases retain unicast loopback, import/API shape, socket controls, and multicast membership coverage.

The measured Node 26.5.0 focused run is 12/15 parity passes, with no compile failures, crashes, or skipped fixtures. The three stable mismatches intentionally diagnose Perry behavior:

1. A second `connect()` while pending or connected is accepted instead of throwing `ERR_SOCKET_DGRAM_IS_CONNECTED`.
2. A non-AbortSignal `signal` option is accepted instead of throwing `ERR_INVALID_ARG_TYPE`.
3. A successful `send()` callback runs synchronously instead of asynchronously.

The work was exercised in coherent batches. Validation/lifecycle probes first exposed unsupported buffer-size validation, repeated bind/close semantics, and active/pre-aborted signal closure. Connection/send probes then separated supported string, typed-array, empty-buffer, default-host, and multiple-send behavior from unsupported offset/length and scatter-gather overloads. The final batch added stable callback-ordering and queue/ref diagnostics before the complete focused rerun.

## Stopping judgment and exclusions

Further upstream ports were stopped where they would duplicate the cases above or cross into a separate runtime/platform feature:

- **Separate runtime work:** Node's new `bindSync()`/`connectSync()`, `Symbol.asyncDispose`, block lists, custom DNS lookup, descriptor/handle binding, send buffer offset/length bounds, scatter-gather arrays, active/pre-aborted signal closure, and exact repeated bind/close return/error semantics.
- **Platform/slow assessment:** `reusePort`, shared ports, cluster/child-process handle transfer, interface-specific IPv6 and link-local addresses, multicast interface/loopback variants, and source-specific multicast beyond the existing smoke fixture.
- **Kernel-sensitive errors:** message-size, out-of-band buffer, receive errors, implicit-bind failure, and address-specific OS error text.
- **Scheduler-sensitive races:** close during bind/lookup/listening, recursive send callbacks, error-quelching races, burst close behavior, ping-pong stress, and unref/cluster process-liveness tests.
- **Redundant upstream variants:** the many connected/unconnected callback, empty-packet, default-host, buffer, typed-array, and multiple-send files are represented by smaller grouped fixtures here.

These exclusions keep the default granular lane deterministic on loopback while preserving the three actionable Perry mismatches as focused regression targets.

## Verification

```text
NODE_BIN=/tmp/node-v26.5.0/bin/node \
PERRY_NO_AUTO_OPTIMIZE=1 \
CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16 \
./run_parity_tests.sh --suite node-suite --module dgram

Parity Pass: 12
Parity Fail: 3
Compile Fail: 0
Crashed: 0
Skipped: 0
Parity Rate: 80.0%

NODE_BIN=/tmp/node-v26.5.0/bin/node \
python3 scripts/node_suite_run.py "$PWD/target/release/perry" "$PWD" dgram

dgram  12  15  80.0  diff=3

cargo fmt --all -- --check
./scripts/check_file_size.sh
python3 -m json.tool test-parity/node_suite_baseline.json
git diff --check
```

No wider module run is required for this baseline ratchet: the executable changes are confined to `node-suite/dgram`, and the baseline runner was measured directly against all 15 dgram fixtures.
