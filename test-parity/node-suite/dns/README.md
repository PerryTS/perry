# `node:dns` granular parity suite

This directory compares deterministic public `node:dns` and `node:dns/promises`
behavior with Node 26.5.0. Each TypeScript file has one contract or one small
record family. The differential runner executes this module sequentially.

## Audited starting point

The six starting fixtures were reviewed before expansion:

- `constants/error-aliases.ts` already covered the full public error-code table.
- `imports/default-export.ts` mixed import identity with a live `resolve4()`
  request. The request was removed because it queried the host nameserver and
  changed between `ECONNREFUSED`, `ENOTFOUND`, and `EBADRESP`.
- `lookup/loopback.ts` uses only the system hosts path and loopback addresses.
  It remains as the broad callback/promise smoke case.
- `resolve/localhost.ts` queried the configured nameserver rather than the hosts
  file. It was removed and replaced with local authoritative-server fixtures.
- `settings/default-result-order.ts` used host-dependent localhost ordering. It
  now tests only shared state, valid values, and invalid-value preservation.
- `settings/servers.ts` only parses and stores server addresses. It remains and
  now reports missing alternate-runtime methods without aborting.

The audit also traced Perry's DNS manifest, native dispatch table,
`crates/perry-runtime/src/dns.rs`, and
`crates/perry-runtime/src/dns_resolver.rs`. Perry implements real wire queries,
but several `Resolver` object, validation, callback-request, TTL, cancellation,
and descriptor contracts still differ from Node.

## Fixed upstream sources

The selection was reviewed on 2026-07-26 against these primary snapshots:

- Node 26.5.0 commit
  [`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb),
  especially
  [`lib/dns.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/dns.js),
  [`internal/dns/utils.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/internal/dns/utils.js),
  [`callback_resolver.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/internal/dns/callback_resolver.js),
  [`promises.js`](https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/internal/dns/promises.js),
  and the
  [`test-dns*` parallel tests](https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel).
- Deno main commit
  [`34c46613cbe20450b74c0e8d4f0fd8f6f781d807`](https://github.com/denoland/deno/tree/34c46613cbe20450b74c0e8d4f0fd8f6f781d807),
  especially
  [`dns_test.ts`](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/tests/unit_node/dns_test.ts)
  and its
  [`node:dns` polyfill](https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/ext/node/polyfills/dns.ts).
- Bun main commit
  [`44f6469e0d4ae93467aa65c7e3bc9001000c7b31`](https://github.com/oven-sh/bun/tree/44f6469e0d4ae93467aa65c7e3bc9001000c7b31),
  especially
  [`node-dns.test.js`](https://github.com/oven-sh/bun/blob/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/test/js/node/dns/node-dns.test.js),
  its selected
  [Node DNS tests](https://github.com/oven-sh/bun/tree/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/test/js/node/test/parallel),
  and
  [`dns.ts`](https://github.com/oven-sh/bun/blob/44f6469e0d4ae93467aa65c7e3bc9001000c7b31/src/js/node/dns.ts).

Node 26.5.0 is the oracle. Deno and Bun results show whether another runtime
made the same choice; they do not weaken the Node contract.

## Covered contracts

- export inventory, default/namespace identity, callback/promise aliases,
  descriptors, function names, and arity;
- literal IPv4/IPv6 lookup, localhost loopback, callback request objects, family
  forms, option accessor order, option validation, falsy hostnames, and
  `util.promisify()` behavior;
- IPv4/IPv6 loopback `lookupService`, port coercion, and argument validation;
- shared default result order, module resolver rebinding, server parsing,
  sparse/accessor arrays, invalid-update preservation, and resolver-local server
  state;
- `Resolver` prototype layout, constructor option access and validation,
  receiver checks, `setLocalAddress`, `setServers`, method validation, active
  cancellation, and idempotent cancellation;
- callback and promise A/AAAA with TTL, ANY, CAA, CNAME, MX, NAPTR, NS, PTR,
  SOA, SRV, TXT, IDNA, rrtype aliases, reverse validation, and DNS error shape.

Record fixtures use `fixtures/local-dns-server.mjs`. It starts a child Node
process, binds an ephemeral UDP loopback port, emits an explicit ready barrier,
returns fixed TEST-NET/documentation records, and closes in `finally`. No
fixture sends a query to an internet nameserver.

## Stopping boundary

The suite stops at 43 fixtures. A fresh review of the fixed Node, Deno, and Bun
trees found no other public contract that was both deterministic, portable,
non-redundant, and reachable through this print-and-diff harness.

Excluded on purpose:

- Node's `test/internet/test-dns-*` files and Bun's public-domain fixtures:
  answers, TTLs, delegation, and availability can change.
- Successful `reverse()` was prototyped against the local server. Perry did not
  settle within 70 seconds, so retaining it would leave cleanup to the harness
  timeout and would not isolate a useful result. Input validation remains.
- Node's two-channel query test was prototyped with two local sockets. Perry did
  not pass the auxiliary-server ready barrier within 30 seconds. Server-state
  independence remains covered without keeping a timeout that mixes child
  process behavior into the DNS result.
- Resolver timeout/retry timing, set-servers-during-query, worker termination,
  perf hooks, snapshots, memory faults, malformed packet counts, TCP fallback,
  and stress cases depend on timers, scheduler order, internals, workers, or
  crash-only harnesses.
- `resolveTlsa()` remains covered by export and method metadata only. It was not
  in the requested record-method set or the selected Deno/Bun suites; adding a
  value-shape case would not supply cross-runtime evidence.
- Exact host `getServers()` defaults, localhost address order, reverse
  hostnames, service names, and non-loopback `lookup()` results depend on OS
  configuration.
- DNS-over-TLS/HTTPS, DNSSEC, cache policy, and transport internals are not
  public `node:dns` contracts in the selected Node suite.

See [EVIDENCE.md](EVIDENCE.md) for the repeated measurements and per-fixture
cross-runtime classification.
