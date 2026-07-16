# `node:dgram` granular parity status

## Upstream evidence

This expansion was reassessed against primary repositories on 2026-07-16:

- Node.js [`608112af`](https://github.com/nodejs/node/tree/608112affae2cf44d2f8a0a6bfe7967193b459c8/test): 82 `test-dgram*` files in `test/parallel` and `test/sequential`.
- Deno [`f8a17c81`](https://github.com/denoland/deno/blob/f8a17c8171569fa2870d740030aaa59c91fdf9ee/tests/node_compat/config.jsonc): 69 selected `parallel/test-dgram*` or `sequential/test-dgram*` entries.
- Bun [`c4fad462`](https://github.com/oven-sh/bun/tree/c4fad462e7dc20e5e9780f848db42e1e2f52186d/test/js/node): 68 copied upstream `test-dgram*` files plus Bun's focused `node-dgram.test.js`.

The Perry fixtures are diagnostic adaptations, not verbatim copies. They use ephemeral ports, loopback addresses, sequential round trips, and deterministic summaries rather than upstream harness helpers or fixed ports.

| Perry area | Representative Node files | Deno selection | Bun copy |
| --- | --- | --- | --- |
| socket and send validation | `test-dgram-createSocket-type.js`, `test-dgram-send-bad-arguments.js`, `test-dgram-send-invalid-msg-type.js` | all 3 | send cases (2/3) |
| bind and close lifecycle | `test-dgram-bind.js`, `test-dgram-bind-error-repeat.js`, `test-dgram-close.js` | all 3 | all 3 |
| AbortSignal | `test-dgram-close-signal.js`, `test-dgram-abort-closed.js` | both | both |
| connection state | `test-dgram-connect.js` | selected | copied |
| send overloads and byte counts | `test-dgram-send-callback-buffer.js`, `test-dgram-bytes-length.js`, `test-dgram-connect-send-callback-buffer.js` | all 3 | all 3 |
| omitted host | `test-dgram-send-default-host.js`, `test-dgram-connect-send-default-host.js` | both | both |
| empty and multiple sends | `test-dgram-send-empty-buffer.js`, `test-dgram-send-empty-array.js`, `test-dgram-implicit-bind.js` | all 3 (empty buffer is Darwin-disabled/flaky) | all 3 |
| callback timing | `test-dgram-send-callback-recursive.js` | selected | copied |
| queue and reference state | `test-dgram-send-queue-info.js`, `test-dgram-ref.js`, `test-dgram-unref.js` | ref/unref (2/3) | ref/unref (2/3) |
| address and connected-send validation | `test-dgram-send-address-types.js`, `test-dgram-send-bad-arguments.js` | both | both |
| legacy and advanced send forms | `test-dgram-sendto.js`, `test-dgram-send-callback-buffer-length.js`, `test-dgram-send-callback-multi-buffer.js` | all 3 | all 3 |
| lookup, errors, and blocking | `test-dgram-custom-lookup.js`, `test-dgram-send-error.js`, `test-dgram-send-cb-quelches-error.js`, `test-dgram-blocklist.js` | lookup/error cases (3/4) | lookup/error cases (3/4) |
| close, bind conflict, and disposal | `test-dgram-close-is-not-callback.js`, `test-dgram-bind-error-repeat.js`, `test-dgram-async-dispose.mjs` | all 3 | all 3 |
| controls and buffer metrics | `test-dgram-setTTL.js`, `test-dgram-multicast-setTTL.js`, `test-dgram-socket-buffer-size.js`, `test-dgram-createSocket-type.js` | TTL/create options (3/4) | TTL cases (2/4) |
| constructor message listener | `test-dgram-udp4.js` | selected | copied |

## Current coverage

The directory contains 38 fixtures: the original 4 broad cases and 34 granular cases added in this expansion.

- `validation/`: socket-type/options matrices, message/list/address validation, connected-send guards, DataView/scatter-gather delivery, legacy `sendto()`, port errors, and offset/length bounds.
- `lifecycle/`: default/port/options bind overloads, bind conflicts and retry state, custom lookup, close arguments and ordering, async-dispose shape, AbortSignal validation, and post-close abort behavior.
- `connection/`: invalid ports, pending/connected state guards, disconnect errors, reconnect state, and connect event/callback ordering.
- `send/`: unconnected and connected overloads, implicit binding, constructor listeners, default address/host behavior, empty buffers/arrays and multiple sends, buffer ranges, block lists, DNS error routing, callback byte counts, and callback asynchrony.
- `metrics/`: constructor and setter buffer sizes, queue metrics, and `ref()`/`unref()` identity before bind, after bind, and after close.
- `control/`: deterministic TTL boundary validation.
- Existing broad cases retain unicast loopback, import/API shape, socket controls, and multicast membership coverage.

The clean Node 26.5.0 focused baseline run is 18/38 parity passes with 20 stable output differences and no compile failures, runtime errors, or skipped fixtures. The differences diagnose:

1. Repeated `connect()` calls and connected range/destination overloads miss `ERR_SOCKET_DGRAM_IS_CONNECTED` guards.
2. Invalid `signal`, `lookup`, buffer-size, and address options/arguments are accepted.
3. Successful `send()` callbacks run synchronously.
4. `sendto()` lacks Node-coded argument errors, scatter-gather and empty arrays are rejected, and buffer offset/length bounds are ignored.
5. `Symbol.asyncDispose`, custom lookup dispatch, and send block lists are not implemented.
6. Constructor buffer sizes are ignored, failed binds leave `address()` reporting `EBADF`, and `close` callback/event ordering differs.
7. `close()` does not return the socket, an empty address is treated as a DNS name, and multicast TTL handles `Infinity` differently.

The work was exercised in coherent batches. The first pass covered core validation, bind/connect lifecycle, basic overloads, callbacks, queue/ref metrics, and AbortSignal. Later batches added 23 cases selected from the larger Node/Deno/Bun corpora: address and advanced-option validation, legacy and connected send forms, custom lookup, close/dispose behavior, bind conflicts and retries, block lists, DNS error routing, buffer metrics/options, TTL boundaries, constructor listeners, message-view/scatter-gather behavior, bounds validation, empty arrays, and implicit-bind state.

## Stopping judgment and exclusions

Further upstream ports were stopped where they would duplicate the cases above or cross into a separate runtime/platform feature:

- **Separate runtime work:** Node's new `bindSync()`/`connectSync()`, descriptor/handle binding, active/pre-aborted signal closure, and repeated bind/close states beyond the deterministic conflict/retry and ordering cases. Surface diagnostics for async disposal, block lists, custom lookup, ranges, and scatter-gather now remain in the suite.
- **Platform/slow assessment:** `reusePort`, shared ports, cluster/child-process handle transfer, IPv6 loopback/link-local/interface-specific cases without a capability guard, multicast interface/loopback variants, and source-specific multicast beyond the existing smoke fixture.
- **Kernel-sensitive errors:** message-size, out-of-band buffer, receive errors, implicit-bind failure, and address-specific OS error text.
- **Scheduler-sensitive races:** close during bind/lookup/listening, recursive send callbacks, error-quelching races, burst close behavior, ping-pong stress, and unref/cluster process-liveness tests.
- **Redundant upstream variants:** the many connected/unconnected callback, empty-packet, default-host, buffer, typed-array, and multiple-send files are represented by smaller grouped fixtures here.

These exclusions keep the default granular lane deterministic on loopback while preserving the 20 actionable Perry differences as focused regression targets.

## Verification

```text
NODE_BIN=/tmp/node-v26.5.0/bin/node \
python3 scripts/node_suite_run.py "$PWD/target/release/perry" "$PWD" dgram

dgram  18  38  47.4  diff=20

cargo fmt --all -- --check
./scripts/check_file_size.sh
python3 -m json.tool test-parity/node_suite_baseline.json
git diff --check
```

No wider module run is required for this module floor: the executable changes are confined to `node-suite/dgram`, and the baseline runner was measured directly against all 38 dgram fixtures. Aggregate metadata remains the last clean full-suite snapshot.
