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

Record fixtures use `fixtures/local-dns-server.mjs`. The `.mjs` extension keeps
the helper out of the runner's recursive `*.ts` fixture discovery and makes its
ES module mode explicit. It starts the same child Node server for every runtime,
binds an ephemeral UDP loopback port, returns fixed TEST-NET/documentation
records, and closes in `finally`. No fixture sends a query to an internet
nameserver.

## Environment and result

- Oracle: Node 26.5.0, commit `bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`.
- Perry runtime: commit `563c35951b347aabac3e093efd9c8b2af8ecd5d9`, built with
  `rustc 1.95.0 (59807616e 2026-04-14)` and `cargo build --release --bin perry`.
- Alternate execution: Deno 2.9.3 and Bun 1.2.18.
- Alternate source review: Deno `34c46613cbe20450b74c0e8d4f0fd8f6f781d807` and
  Bun `44f6469e0d4ae93467aa65c7e3bc9001000c7b31`.

Three complete Node rounds ran all 43 fixtures with zero errors, crashes, or
timeouts and byte-identical aggregate SHA-256
`e18b3a7e82b9309c1f8db862d25a5ee1ec2210a61a4f0ad09582ebff948a2f2d`. Three
complete focused Perry runs produced the same **17 pass / 26 diff / 0 compile
failure / 0 crash / 0 timeout** result after active cancellation idempotence was
added. The baseline records `17/43`.

One complete alternate-runtime pass produced:

- Deno: 21 exact matches and 22 diffs; no error, crash, or timeout.
- Bun: 13 exact matches and 30 diffs; no error, crash, or timeout.

## Stable Perry differences

- Module surface: the `promises` property is a data property rather than Node's
  lazy getter; public functions use different names and arities.
- Lookup: callback requests return `undefined`; callback checks, falsy-host
  errors, string families, option getter order, validation, and promisification
  differ.
- Lookup service: numeric-string promise coercion and argument checks differ.
- Resolution: TTL objects, typed record fields, callback request objects, and
  enumerable DNS error fields differ. A/AAAA values, ANY values after key
  canonicalization, IDNA, name records, and TXT records match.
- Resolver: active `cancel()` reports `ETIMEOUT` instead of `ECANCELLED`;
  constructor checks, prototype layout, method metadata, resolve validation, and
  local-address validation differ.
- Settings: default-resolver method rebinding, sparse/accessor server arrays,
  and one bracketed IPv6 normalization case differ.

## Per-fixture classification

`pass` and `match` mean exact stdout and exit-code parity with Node 26.5.0.
`diff` means the fixture completed but exposed a stable contract difference.

| Fixture                                  | Perry | Deno  | Bun   |
| ---------------------------------------- | ----- | ----- | ----- |
| `constants/error-aliases.ts`             | pass  | diff  | match |
| `imports/aliases.ts`                     | pass  | diff  | diff  |
| `imports/default-export.ts`              | pass  | match | match |
| `imports/descriptors.ts`                 | diff  | diff  | diff  |
| `imports/export-inventory.ts`            | pass  | diff  | diff  |
| `imports/function-metadata.ts`           | diff  | diff  | diff  |
| `lookup-service/ipv6-loopback.ts`        | pass  | match | diff  |
| `lookup-service/port-coercion.ts`        | diff  | diff  | diff  |
| `lookup-service/validation.ts`           | diff  | match | match |
| `lookup/callback-validation.ts`          | diff  | match | match |
| `lookup/falsy-hostname.ts`               | diff  | diff  | diff  |
| `lookup/family-forms.ts`                 | diff  | diff  | diff  |
| `lookup/ip-literals-callback.ts`         | diff  | match | diff  |
| `lookup/ip-literals-promises.ts`         | pass  | match | match |
| `lookup/loopback.ts`                     | pass  | match | match |
| `lookup/options-accessors.ts`            | diff  | diff  | diff  |
| `lookup/options-validation.ts`           | diff  | match | match |
| `lookup/promisify.ts`                    | diff  | diff  | diff  |
| `resolve/address-records.ts`             | diff  | match | diff  |
| `resolve/any-records.ts`                 | pass  | match | match |
| `resolve/errors.ts`                      | diff  | diff  | diff  |
| `resolve/idna.ts`                        | pass  | match | diff  |
| `resolve/name-records.ts`                | pass  | diff  | match |
| `resolve/reverse-validation.ts`          | diff  | diff  | diff  |
| `resolve/rrtype-aliases.ts`              | diff  | match | diff  |
| `resolve/structured-records.ts`          | diff  | diff  | diff  |
| `resolve/txt-record.ts`                  | pass  | match | diff  |
| `resolver/cancel-active.ts`              | diff  | match | diff  |
| `resolver/cancel-idempotent.ts`          | pass  | match | match |
| `resolver/constructor-validation.ts`     | diff  | match | diff  |
| `resolver/method-metadata.ts`            | diff  | diff  | diff  |
| `resolver/options-accessors.ts`          | pass  | diff  | diff  |
| `resolver/prototype.ts`                  | diff  | diff  | diff  |
| `resolver/receiver-validation.ts`        | pass  | match | diff  |
| `resolver/resolve-receiver.ts`           | pass  | match | diff  |
| `resolver/resolve-validation.ts`         | diff  | match | diff  |
| `resolver/set-local-address.ts`          | diff  | diff  | diff  |
| `resolver/set-servers-validation.ts`     | pass  | match | match |
| `settings/default-resolver-rebinding.ts` | diff  | diff  | diff  |
| `settings/default-result-order.ts`       | pass  | diff  | diff  |
| `settings/servers-array-semantics.ts`    | diff  | match | match |
| `settings/servers-normalization.ts`      | diff  | diff  | match |
| `settings/servers.ts`                    | pass  | diff  | diff  |

## Commands

```sh
cargo build --release --bin perry
NODE_BIN="$HOME/.nvm/versions/node/v26.5.0/bin/node" \
  python3 scripts/node_suite_run.py target/release/perry "$PWD" dns
python3 -m json.tool test-parity/node_suite_baseline.json >/dev/null
```

Local authoritative-server cases need permission to bind ephemeral loopback UDP
ports and spawn the helper Node process.

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
- The valid callback `Resolver.resolve(hostname, callback)` default-rrtype
  overload was prototyped against the local server. Node and Deno settled, but
  Perry did not invoke the callback. Retaining it would require an arbitrary
  timeout or fire-and-forget cleanup; argument validation remains covered.
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
