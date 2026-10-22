# `node:net` granular parity suite

This directory compares Perry with Node.js 26.5.0. Each TypeScript file prints
one focused contract. Network cases use loopback, ephemeral ports, event or
callback barriers, and `finally` cleanup.

## Scope

The 47 fixtures cover:

| Area            | Files | Contracts                                                                                                                                  |
| --------------- | ----: | ------------------------------------------------------------------------------------------------------------------------------------------ |
| `classes`       |     8 | `BlockList` and `SocketAddress` identity, construction, parsing, validation, JSON, IPv4, IPv6, and class inputs                            |
| `connection`    |    12 | connect overloads, state, address metadata, data, event order, half-close state, IPv6 loopback, counters, corking, backpressure, and abort |
| `exports`       |     5 | exports, descriptors, aliases, classes, prototypes, and `BoundSocket` presence                                                             |
| `ip`            |     1 | IPv6 zone identifier classification                                                                                                        |
| `method-values` |     2 | socket/server methods and async disposal                                                                                                   |
| `server`        |     5 | initial state, address lifecycle, listen overloads, connection counts, and paused accepts                                                  |
| `socket`        |     4 | initial state, chainable controls, destroy/reset, and type of service                                                                      |
| `validation`    |    10 | constructor, connect, listen, port, timeout, auto-family, and receiver checks                                                              |

The suite started with 13 fixtures. This change adds 36 and removes two broad or
nondeterministic fixtures, leaving 47. `server/connection-state-limits.ts` used
a 300 ms sleep. Deno also leaves an over-limit client pending without a `drop`
or `connection` event, so the case has no portable completion barrier.
`server/get-connections.ts` keeps deterministic connection-count coverage. The
mixed `classes/blocklist-socketaddress.ts` fixture is replaced by focused class
fixtures that each print one contract.

## Run

```sh
NODE_BIN=/path/to/node-v26.5.0/bin/node \
python3 scripts/node_suite_run.py "$PWD/target/release/perry" "$PWD" net

PATH=/path/to/node-v26.5.0/bin:$PATH \
./run_parity_tests.sh --suite node-suite --module net
```

`EVIDENCE.md` fixes the source revisions, maps fixtures to upstream contracts,
records cross-runtime results, and explains the stopping point.

## Mutable half-open policy

`socket/allow-half-open-data-property.ts` checks the ordinary own data property,
constructor normalization, arbitrary assignments, and deletion.
`connection/allow-half-open-mutation.ts` uses owned numeric loopback connections
to check both mutation directions, strict-false automatic ending, listener
mutation, accepted-child defaults, and getter/destroy/reconnect behavior.
Run either with `--suite node-suite --module net --filter <fixture-name>`.
These cases do not certify the separate legacy `new tls.TLSSocket` constructor
or complete NET/TLS compatibility.

The published-main T94 port preserves upstream Socket metadata behavior.
Fresh shared B95 baseline and paired functional/independent verification
remain pending; see `EVIDENCE.md` for the source-only port boundary.

2026-10-21: T101 recorded three raw-Node-exact half-open targets on NET_F99.
See the dated self-verification record in `EVIDENCE.md` for its source and limits.

## BlockList rule-category emission

`classes/block-list-json-validation.ts` retains its original validation cases
and adds all six address/subnet/range insertion permutations, with at most one
address per new history, plus repeated subnet/range metadata. Direct mutators
and JSON imports expose both `.rules` and `toJSON()`, membership and independent
snapshots. Node 26.12.1's native
[`ListRules`](https://github.com/nodejs/node/blob/v26.12.1/src/node_sockaddr.cc#L756-L786)
emits addresses, then subnet metadata, then ranges. The shared serializer now
follows that category order while retaining existing per-category order and
duplicates. Arbitrary native address-map traversal and repeated or mapped-family
address deduplication remain unresolved; this is not general BlockList parity.

```sh
./run_parity_tests.sh --suite node-suite --module net --filter node-suite/net/classes/block-list-json-validation
```

Use the full fixture ID: the shorter `block-list-json` substring also selects
this validation fixture.

The category-only repair has coherent candidate-build and paired functional
verification evidence: four self-verification calls exited 0, and independent
baseline/candidate calls exited 1/1/1/0 and 0/0/0/0 respectively, with every
candidate output raw-Node-exact. These results are bound to the measured
candidate, not a full NET or latest-main certification.

## Successful end overloads

`connection/end-callback-order.ts` retains the original successful shutdown
scenario and compares callback-only, data/callback, data/UTF-8/callback, and
repeated callback registrations while connecting and after connection. Owned
numeric-loopback cases are bounded and print received bytes, callback counts,
finish/close counts, and event order. Reusing one callback for two `end` calls
must retain both registrations; this is not callback-identity deduplication.
This fixture does not certify other string encodings or native shutdown
cancellation/lifetime behavior.

## Outbound string encodings

`connection/string-write-encoding.ts` compares dynamic encoding arguments for
`write` and `end`, while connected and connecting. It covers inline/heap
strings, UTF-8 aliases, hex, base64/base64url, latin1/binary, ascii, UTF-16LE/UCS-2
aliases, malformed input, falsy UTF-8 defaults, synchronous unknown-encoding
errors, callback/finish/close counts, and decoded-byte cork accounting.
Encoding validation precedes transport state and applies to binary chunks too;
valid encodings leave Buffer/Uint8Array bytes unchanged. An owned TLS journey
wraps an existing numeric-loopback socket using the checked-in TLS certificates
and explicit `rejectUnauthorized: false` only for that fixture connection.
This does not certify receive-side `setEncoding`, HTTP, direct TLSSocket
construction, native shutdown cancellation, or complete NET/TLS compatibility.
