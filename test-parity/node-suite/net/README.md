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
