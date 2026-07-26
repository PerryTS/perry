# `node:dns` parity evidence

## Environment and result

- Oracle: Node 26.5.0, commit `bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`.
- Perry: branch `test/expand-node-dns-parity`, built with
  `cargo build --release --bin perry`.
- Alternate execution: Deno 2.9.3 and Bun 1.2.18.
- Alternate source review: Deno `34c46613cbe20450b74c0e8d4f0fd8f6f781d807` and
  Bun `44f6469e0d4ae93467aa65c7e3bc9001000c7b31`.

Three complete Node rounds ran all 43 fixtures with zero errors, crashes, or
timeouts and byte-identical aggregate SHA-256
`c08378de2b06928db9a288ddfc6515bdbc401d814dba853a130d09adfda5a412`. Three
complete focused Perry runs produced the same **18 pass / 25 diff / 0 compile
failure / 0 crash / 0 timeout** result. The baseline records `18/43`.

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
