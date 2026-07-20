# node:cluster granular parity suite

Deterministic differential cases for Perry's `node:cluster` compatibility layer.
The module is in `scripts/node_suite_run.py`'s sequential slow lane.

The suite uses self-contained primary/worker programs, ephemeral TCP ports,
event or IPC completion barriers, unref'ed watchdogs, and cleanup on every
terminal path. It does not depend on the historical broad parity files.

## Coverage

- module/default export identity, role flags, constants, descriptors, and
  EventEmitter/Worker prototype shape;
- setup defaults, cumulative snapshots, aliases, setup events, fork option
  forwarding, and synchronous validation;
- fork registry/id/respawn behavior and primary/worker role state;
- worker online/message/disconnect/exit ordering, payloads, methods, channel,
  send callbacks/errors, kill metadata, and cluster-wide disconnect;
- JSON and advanced IPC serialization;
- single-worker TCP listening descriptors and request/response behavior on
  port 0.

See [STATUS.md](./STATUS.md) for source provenance, measured gaps, and the
categories intentionally stopped before flaky or infrastructure-heavy tests.
