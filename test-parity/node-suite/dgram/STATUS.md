# `node:dgram` granular parity status

## Scope and oracle

This suite uses Node.js 26.5.0 as the exact oracle. The audit covers public
`node:dgram` behavior that can run with loopback addresses, ephemeral ports,
small payloads, and bounded event-loop turns. It does not use
`test-files/test_parity_*.ts`.

The fixtures adapt upstream contracts instead of copying the upstream harness.
Every network fixture closes its sockets and avoids Internet access, fixed
ports, host interfaces, real multicast traffic, and arbitrary sleeps.

## Pinned upstream evidence

- Node.js [`v26.5.0` / `bebd1b8d`](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb):
  [`lib/dgram.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/dgram.js),
  [`lib/internal/dgram.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/internal/dgram.js),
  and 82 `test-dgram*` files under `test/parallel` and `test/sequential`.
- Deno [`34c46613`](https://github.com/denoland/deno/tree/34c46613cbe20450b74c0e8d4f0fd8f6f781d807):
  [`ext/node/polyfills/dgram.ts`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/ext/node/polyfills/dgram.ts),
  [`tests/unit_node/dgram_test.ts`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/tests/unit_node/dgram_test.ts),
  and [`tests/node_compat/config.jsonc`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/tests/node_compat/config.jsonc).
  The config enables 61 dgram tests and records eight more as platform-limited.
- Bun [`43d60f69`](https://github.com/oven-sh/bun/tree/43d60f69c95a9f31591165816ced29b83e94673e):
  79 copied `test-dgram*` files plus
  [`node-dgram.test.js`](https://github.com/oven-sh/bun/blob/43d60f69c95a9f31591165816ced29b83e94673e/test/js/node/dgram/node-dgram.test.js).
  Its copied corpus omits Node 26.5.0's `bindSync`, `connectSync`, default-literal
  lookup, and raw-handle tests.
- The audit checked Perry support in `crates/perry-runtime/src/dgram.rs` and
  `crates/perry-runtime/src/dgram/{net,ops,thunks}.rs`. Work owned by `net`,
  `dns`, `cluster`, `worker_threads`, and `async_hooks` remains outside this
  suite.

## Fixture map

`New` marks the 16 fixtures added by this audit. All source names refer to the
pinned Node 26.5.0 tree above.
`match`, `diff`, and `error` record direct output and exit-status comparisons
against that oracle. The pinned Deno polyfill/config and Bun copied corpus above
show whether each runtime also carries the upstream test family.

| Fixture | New | Contract | Primary Node evidence | Deno 2.9.3 | Bun 1.2.18 |
| --- | --- | --- | --- | --- | --- |
| `api/export-shape.ts` | yes | enumerable ESM/CJS export set | `lib/dgram.js` exports | match | match |
| `api/receiver-validation.ts` | yes | method `call` receiver validation | `lib/dgram.js` state access | match | match |
| `api/socket-aliases.ts` | yes | EventEmitter aliases and absent `hasRef` | `lib/dgram.js` inheritance | match | match |
| `api/socket-class.ts` | yes | construction and prototype identity | `lib/dgram.js` `Socket` setup | match | diff |
| `api/socket-descriptors.ts` | yes | method and socket type descriptors | `lib/dgram.js` prototype/type setup | diff | match |
| `connection/connect-ordering.ts` | no | connect event/callback order, receiver, args, remote state | `test-dgram-connect.js` | match | match |
| `connection/connect-sync.ts` | yes | synchronous connect, remote address, deferred event | `test-dgram-connect-sync.js` | diff | diff |
| `connection/state-transitions.ts` | no | pending/connected guards, disconnect, reconnect | `test-dgram-connect.js` | match | match |
| `control/ttl-validation.ts` | no | unicast and multicast TTL boundaries | `test-dgram-setTTL.js`, `test-dgram-multicast-setTTL.js` | error | error |
| `default-import-shape.ts` | no | default export identity and broad method surface | `lib/dgram.js` exports | diff | diff |
| `ipv6-loopback.ts` | yes | guarded IPv6 bind, address, send, `rinfo` | `test-dgram-address.js` | match | match |
| `lifecycle/abort-signal.ts` | no | invalid signal and abort after close | `test-dgram-close-signal.js`, `test-dgram-abort-closed.js` | error | match |
| `lifecycle/async-dispose-lifecycle.ts` | yes | async disposal closes before its promise resolves | `test-dgram-async-dispose.mjs` | match | match |
| `lifecycle/async-dispose-shape.ts` | no | `Symbol.asyncDispose` surface | `test-dgram-async-dispose.mjs` | error | match |
| `lifecycle/bind-conflict.ts` | no | ephemeral loopback `EADDRINUSE` event | `test-dgram-bind-error-repeat.js` | match | diff |
| `lifecycle/bind-error-details.ts` | yes | bind error `address` and `port` fields | `lib/dgram.js` bind error path | match | diff |
| `lifecycle/bind-error-retry.ts` | no | repeated bind errors and post-error address state | `test-dgram-bind-error-repeat.js` | diff | diff |
| `lifecycle/bind-overloads.ts` | no | loopback port/address and options overloads with callback receiver | `test-dgram-bind.js` | match | match |
| `lifecycle/bind-state.ts` | yes | return identity and already-bound validation order | `test-dgram-bind.js` | match | match |
| `lifecycle/bind-sync.ts` | yes | synchronous bind, returned address, deferred event | `test-dgram-bind-sync.js` | diff | diff |
| `lifecycle/close-arguments.ts` | no | ignored non-function callback and return identity | `test-dgram-close-is-not-callback.js` | error | match |
| `lifecycle/close-ordering.ts` | no | close event/callback order, receiver, and arity | `lib/dgram.js` close path | match | match |
| `lifecycle/closed-state.ts` | yes | repeated close and safe post-close operations | `lib/dgram.js` `healthCheck()` | diff | diff |
| `lifecycle/custom-lookup.ts` | no | socket-option lookup dispatch | `test-dgram-custom-lookup.js` | error | match |
| `lifecycle/default-lookup.ts` | yes | own-family literal bypass and mismatched-family DNS dispatch | `test-dgram-default-lookup-ip.js` | error | diff |
| `metrics/buffer-sizes.ts` | no | bound/unbound buffer access and range validation | `test-dgram-socket-buffer-size.js` | error | diff |
| `metrics/create-socket-buffer-options.ts` | no | constructor receive/send buffer sizes | `test-dgram-createSocket-type.js` | match | diff |
| `metrics/queue-and-ref.ts` | no | zero queue metrics and ref/unref identities by state | `test-dgram-send-queue-info.js`, `test-dgram-ref.js`, `test-dgram-unref.js` | error | match |
| `multicast-membership.ts` | no | membership argument and closed-state validation only | `test-dgram-membership.js` | match | diff |
| `send/blocklist.ts` | no | send/connect block-list errors | `test-dgram-blocklist.js` | diff | diff |
| `send/buffer-range.ts` | no | offset/length payload and callback byte count | `test-dgram-send-callback-buffer-length.js` | match | match |
| `send/callback-ordering.ts` | no | callback asynchrony, values, and delivery | `test-dgram-send-callback-recursive.js`, `test-dgram-bytes-length.js` | match | match |
| `send/callback-receiver.ts` | yes | send callback receiver, args, and arity | `lib/dgram.js` `doSend()` / `afterSend()` | diff | match |
| `send/connected-overloads.ts` | no | connected string and typed-array sends | connected callback-buffer tests | match | match |
| `send/create-socket-listener.ts` | no | constructor message listener and `rinfo` | `test-dgram-udp4.js` | match | match |
| `send/default-address-values.ts` | no | empty, null, and undefined address normalization | empty-address callback tests | match | match |
| `send/default-host.ts` | no | omitted udp4 host for connected/unconnected sends | default-host tests | match | match |
| `send/empty-and-multiple.ts` | no | empty packet and sequential small sends | empty-buffer and implicit-bind tests | match | match |
| `send/empty-array.ts` | no | empty scatter list acceptance and byte count | `test-dgram-send-empty-array.js` | match | match |
| `send/error-routing.ts` | no | callback suppresses the error event | `test-dgram-send-cb-quelches-error.js` | match | match |
| `send/implicit-bind-state.ts` | no | two implicit binds get valid distinct ports | `test-dgram-implicit-bind.js` | match | match |
| `send/ipv6-default-host.ts` | yes | omitted udp6 host resolves to `::1` | `test-dgram-udp6-send-default-host.js` | match | match |
| `send/overloads.ts` | no | unconnected string and typed-array sends | callback-buffer tests | match | match |
| `send/scatter-copy.ts` | yes | input array ownership after `send()` | `test-dgram-send-multi-buffer-copy.js` | match | match |
| `socket-controls.ts` | no | broadcast, TTL, loopback, interface, buffer controls | set/control and buffer-size tests | error | diff |
| `unicast-loopback.ts` | no | broad IPv4 bind/connect/send/disconnect lifecycle | `test-dgram-address.js`, `test-dgram-connect.js`, `test-dgram-udp4.js` | match | diff |
| `validation/address-arguments.ts` | no | address type validation | `test-dgram-send-address-types.js` | match | match |
| `validation/buffer-bounds.ts` | no | offset and length bounds | `test-dgram-send-bad-arguments.js` | match | match |
| `validation/connected-send-arguments.ts` | no | destination/range forms on connected sockets | `test-dgram-send-bad-arguments.js` | match | match |
| `validation/create-socket-advanced-options.ts` | no | lookup, buffer-size, and block-list validation | `lib/dgram.js`, `test-dgram-createSocket-type.js` | diff | diff |
| `validation/create-socket-options.ts` | no | socket-type overload matrix | `test-dgram-createSocket-type.js` | error | match |
| `validation/message-views.ts` | no | DataView and mixed scatter-gather sends | multi-buffer callback tests | match | match |
| `validation/send-arguments.ts` | no | message/list/port validation | `test-dgram-send-bad-arguments.js`, `test-dgram-send-invalid-msg-type.js` | match | match |
| `validation/sendto.ts` | no | legacy `sendto()` validation codes | `test-dgram-sendto.js` | match | match |

Nine existing fixtures add callback receiver data, block-list option validation,
and loopback-only lookup/error routing. Every fixture that creates a socket now
closes it from `finally`, and each fixture removes persistent listeners after
its contract settles. `multicast-membership.ts` now checks validation only; it no
longer joins a real multicast group.

## Repeated results

All totals were stable across three consecutive runs on 2026-07-27. Perry
ran without another process reading or rebuilding `target/perry-auto-*`; shared
cache work can invalidate a measurement. Three consecutive focused runs
produced the clean Perry row below with no compile failure, crash, or timeout.

| Runtime | Version | Pass | Diff | Error | Compile/crash/timeout | Unstable |
| --- | --- | ---: | ---: | ---: | ---: | ---: |
| Node oracle | 26.5.0 | 54 | 0 | 0 | 0 | 0 |
| Perry focused runner | current branch | 18 | 36 | 0 | 0 | 0 |
| Deno direct | 2.9.3 | 35 | 9 | 10 | 0 | 0 |
| Bun direct | 1.2.18 | 37 | 16 | 1 | 0 | 0 |

The Deno and Bun rows compare direct runtime output and exit status with the
same Node oracle output. Deno's 10 stable errors exit non-zero rather than print
Node's result. Bun's one stable error is the existing multicast-TTL `Infinity`
case. Neither runtime timed out.

Perry's 18 exact matches cover IPv4 and IPv6 loopback, bind overloads and
conflicts, connect ordering, buffer access, zero queue/ref state, core send
overloads, default hosts, sequential sends, implicit binding, socket controls,
socket-type validation, and basic send validation.

The 36 stable Perry differences group into:

1. export/class/prototype/descriptors and receiver validation;
2. `bindSync()` / `connectSync()` and pending/already-bound state guards;
3. signal validation/closure and async disposal;
4. bind error metadata, failed-bind address state, close return/order/state,
   and default/custom lookup dispatch;
5. constructor buffer options, block-list options, and membership closed-state
   codes;
6. TTL `Infinity`, range slicing, empty/scatter arrays, array ownership,
   callback timing/receiver, and empty-address normalization;
7. address, buffer-bound, connected-send, advanced-option, and `sendto()`
   validation codes.

## Exclusions and stopping criterion

The final Node/Deno/Bun pass found no further contract that was both portable,
deterministic, in scope, and not already represented. The audit excludes:

- descriptor/handle binding, fd ownership, raw UDP handles, and kernel fault
  injection;
- `reusePort`, shared-port distribution, cluster, child processes, and
  `worker_threads` socket transfer;
- AsyncLocalStorage propagation and async provider IDs;
- Internet names, external DNS, non-loopback interfaces, fixed ports, and real
  multicast/broadcast delivery;
- interface-specific IPv6, link-local scope IDs, dual-stack port sharing, and
  source-specific multicast delivery;
- message-size/OOB/receive faults, resource pressure, signals, GC, and stress;
- pre-aborted and active-socket AbortSignal closure because Perry cannot expose
  completion without a scheduler race or timeout;
- close-during-lookup/bind races, recursive callback races, burst batch
  delivery, process-liveness timers, and ping-pong tests;
- queue-depth assertions that need Node's private `--test-udp-no-try-send`
  flag; the portable zero-state queue contract remains covered.

These boundaries assign DNS resolver behavior to `dns`, socket distribution to
`cluster`/`worker_threads`, stream semantics to `net`, and async context to
`async_hooks`.

## Verification

```text
NODE_BIN=/private/tmp/node-v26.5.0-bin/bin/node \
python3 scripts/node_suite_run.py "$PWD/target/release/perry" "$PWD" dgram

dgram  18  54  33.3  diff=36
```

The table and repeated direct-runtime totals come from this harness:

```python
import pathlib
import subprocess

root = pathlib.Path.cwd()
base = root / "test-parity/node-suite/dgram"
paths = sorted(base.rglob("*.ts"))
node = "/private/tmp/node-v26.5.0-bin/bin/node"
oracle = {}

for path in paths:
    result = subprocess.run(
        [node, path], capture_output=True, text=True, timeout=10, check=False
    )
    oracle[path] = (result.returncode, result.stdout.rstrip("\n"))

for name, command in {
    "deno": ["deno", "run", "--allow-net"],
    "bun": ["bun"],
}.items():
    first = {}
    for run in range(1, 4):
        counts = {
            "match": 0, "diff": 0, "error": 0,
            "timeout": 0, "unstable": 0,
        }
        for path in paths:
            try:
                result = subprocess.run(
                    command + [path], capture_output=True, text=True,
                    timeout=10, check=False
                )
            except subprocess.TimeoutExpired:
                counts["timeout"] += 1
                continue
            value = (result.returncode, result.stdout.rstrip("\n"))
            status = "error" if result.returncode else (
                "match" if value == oracle[path] else "diff"
            )
            if run == 1:
                first[path] = value
                print(name, path.relative_to(base), status, sep="\t")
            elif value != first[path]:
                status = "unstable"
            counts[status] += 1
        print(name, f"run={run}", counts, sep="\t")
```

```text

cargo fmt --all -- --check
./scripts/check_file_size.sh
python3 -m json.tool test-parity/node_suite_baseline.json
git diff --check
```

`cargo fmt` currently stops on unchanged `main` formatting in
`crates/perry/src/commands/compile/collect_modules.rs`. The file-size check also
stops on unchanged `crates/perry-runtime/src/object/mod.rs` (2039 lines) and
`crates/perry-stdlib/src/readline.rs` (2066 lines). `deno check` cannot resolve
the repository's matching `npm:@types/node` package from the local install;
`deno fmt --check` passes for every changed fixture.

Only the dgram floor changed in `test-parity/node_suite_baseline.json`, from
18/38 to the clean repeated 18/54 measurement. Aggregate metadata still
describes the last full-suite run.
