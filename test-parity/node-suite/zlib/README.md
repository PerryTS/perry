# `node:zlib` granular parity suite

This suite compares small, fixed `node:zlib` contracts. Node 26.5.0 is the
oracle. Each fixture prints semantic state or error shape; no fixture compares
backend-specific compressed bytes.

## Chunk-size contract

`options/chunk-size-contract.ts` covers the pinned Node 26.12.1 numeric
contract through named, namespace, and captured factories, sibling DEFLATE
factories, and options-consuming sync calls. Undefined and NaN use the default;
null is a type error, infinities require a finite-number range error, and finite
sizes retain the 64-byte minimum (including accepted fractions). Throwing getters
and small high-water-mark differential controls preserve the existing boundary.
No large-allocation/OOM probes, decoder modes, dictionary application, or general
getter-order coverage are added. Historical audit measurements below are unchanged.

## Callback settlement contract

The callback-shape and options-overload fixtures also pin the Node 26.12.1
public engine receiver: genuine Gzip/Gunzip and Transform identity, consumed
`bytesWritten`, and inherited `on`/`emit`/`removeListener` behavior. Success
receives `(null, Buffer)`; failure receives exactly one Error argument. Invalid
gzip magic is a data error once two bytes arrive; a lone byte, a short method
prefix, and a valid header without a complete payload remain truncated-input
errors. Existing observations precede these additional checks.

The callback implementation uses one real engine and one ZLIB provider across
WRITE and a separately queued empty FINAL. It snapshots only the previously
resolved effective level, preserving original validation and copy/getter order
without newly applying ignored options. Codec release and owned input/output
retirement precede user callbacks, including reentrant or throwing callbacks.
Abandoned WRITE after-hook and construction/getter throws retain the existing
GC-owned fallback, not an unconditional prompt-release guarantee. Runtime
ownership and copy/getter-order witnesses accompany these public fixtures;
they do not assert the stronger exceptional prompt-release claim.

## Gzip constructor option phases

`options/constructor-option-values.ts` uses the pinned Node 26.12.1
[`Zlib` and `ZlibBase` constructors](https://github.com/nodejs/node/blob/v26.12.1/lib/zlib.js).
Eight finite public `createGzip()` cases observe conditional `windowBits` reads,
dictionary-before-chunk validation, first-validated compression levels across
later enumerable option copying, and thrown-sentinel identity. Accepted streams
report genuine Gzip identity, gzip header XFL, `bytesWritten`, end and close;
they do not compare backend-specific DEFLATE bytes.

The Gzip-only constructor path retains rooted codec options, owns the same codec
before base/Transform getters, and passes a real enumerable option copy to the
existing Transform initializer. Other codecs and shared sync/callback validators
keep their previous schedule. Gzip dictionary acceptance still does not apply it
to the backend. Exceptional construction retains the existing GC-owned fallback;
this change does not promise unconditional prompt release after getter throws.

`options/constructor-getter-phases.ts` remains the original full-hex diagnostic,
byte-for-byte unchanged. Its known compression-backend residual is separate from
this semantic fixture; preserving it does not mark it green. The existing
`options/validation-order.ts` is also unchanged. Historical audit measurements
and exclusions below describe their original fixture roster, not these additions.

## DEFLATE preset dictionaries

`dictionary/deflate-roundtrip.ts`, `dictionary/source-types.ts`, and
`dictionary/stream-application.ts` exercise real zlib-wrapped preset dictionary
application, not just constructor validation. `deflateSync` and `createDeflate`
use the already-linked zlib-ng backend only when a dictionary is supplied;
ordinary compression retains its existing backend and output. `inflateSync`
and `createInflate` supply the owned dictionary when the decoder requests it.
Buffer, TypedArray, DataView and ArrayBuffer sources retain their byte range.
Stream reset retains the selected dictionary, without retaining borrowed JS
pointers. Native codec working allocations remain in the payload buffer ledger.

`dictionary/deflate-wrong-dictionary.ts` pins the Node 26.12.1 errors for missing
and wrong dictionaries (`Z_NEED_DICT`, with distinct messages), a sliced DataView,
and reset followed by split input. Brotli dictionary application remains separate
unsupported codec work.

## Zstd dictionary history

`dictionary/zstd-roundtrip.ts` covers direct and captured synchronous APIs and
stream constructors, sliced DataView/TypedArray dictionaries, reset followed by
split input, and ordinary no-history controls. Sync calls read only the dictionary
option; stream constructors retain their existing validation schedule. Each codec
owns an immediate view-bounded byte snapshot, and reset reloads it into the new
native context through the existing payload owner and allocation accounting.
Actual backend error numbers determine Zstd error codes, not message matching.
A wrong raw-content dictionary can succeed with incorrect plaintext; the fixture
does not invent a universal wrong-dictionary error. Missing-history rejection
distinguishes real dictionary application from an ordinary roundtrip.
Callback option capture and settlement are unchanged by this sync/stream work.

## Brotli one-shot output limits

`options/max-output-length.ts` covers namespace, named and captured one-shot
Brotli decompression. Both synchronous and callback routes enforce
`maxOutputLength` before appending beyond the cap, accept an exact-size result,
and preserve the ordinary uncapped behavior and compressed input bytes.
Over-limit results use a managed `RangeError` with `ERR_BUFFER_TOO_LARGE`;
callbacks receive no output while retaining their genuine codec receiver.
This policy does not change DEFLATE dictionaries or general stream options.

## Locked sources

- Node 26.5.0 commit [`bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb`][node-tree]:
  [`lib/zlib.js`][node-lib] and all 68 zlib-named tests under `test/`.
- Deno commit [`34c46613cbe20450b74c0e8d4f0fd8f6f781d807`][deno-tree]:
  [`tests/unit_node/zlib_test.ts`][deno-test],
  [`ext/node/polyfills/zlib.js`][deno-lib], and the native zlib ops.
- Bun commit [`43d60f69c95a9f31591165816ced29b83e94673e`][bun-tree]:
  [`src/js/node/zlib.ts`][bun-lib], [`test/js/node/zlib/`][bun-zlib-tests], and
  Bun's vendored Node zlib tests.
- Local runtimes: Node 26.5.0, Bun 1.2.18, and Deno 2.9.3.

## Measurements

The audit grew the suite from 58 to 92 fixtures. Three independent Node runs
produced 92 clean exits and the same ordered stdout SHA-256:
`9fca491aaffd2caea529f91f2e41b13b76dd773e049e25c39ac4fad6aa430658`.

| Runtime     |               Result | Stable differences                        |
| ----------- | -------------------: | ----------------------------------------- |
| Node 26.5.0 |   92/92 oracle exits | none                                      |
| Perry       | 62 matches, 30 diffs | no compile failures, crashes, or timeouts |
| Bun 1.2.18  |  86 matches, 6 diffs | no errors, crashes, or timeouts           |
| Deno 2.9.3  |  83 matches, 9 diffs | no errors, crashes, or timeouts           |

Perry's 30 diffs cover callback context, immutable exports, constructor and
prototype identity, `info`, CRC seed validation, dictionary application,
trailing input, option validation and getter order, exact stream state,
`bytesWritten`, multi-codec stream behavior, flush order, and truncated input.
Four new contracts already match: `SharedArrayBuffer` input, semantic Brotli
params, level/strategy defaults and ranges, and reset/params before the first
write.

Bun differs on Brotli and Zstd dictionary application, option getter order, the
legacy `bytesRead` alias, `crc32(ArrayBuffer)`, and `rejectGarbageAfterEnd`.
Deno differs on all five dictionary application fixtures, invalid Brotli input,
legacy alias enumerability, getter order, and `rejectGarbageAfterEnd`.

Run the focused Perry comparison with:

```sh
NODE_BIN=/path/to/node-v26.5.0/bin/node \
  python3 scripts/node_suite_run.py target/release/perry "$PWD" zlib
```

## Added contracts

Every row names the exact upstream evidence and the gap in the old 58-fixture
suite.

| Fixture                                  | Source                                                                                                               | Missing contract                                                                                      |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------- |
| `async/error-callback-shape.ts`          | [`lib/zlib.js`][node-lib], [`test-zlib-from-gzip-with-trailing-garbage.js`][node-trailing]                           | Async decode errors call back later with one error argument, no output, and the engine receiver.      |
| `async/options-overload-and-callback.ts` | [`test-zlib-convenience-methods.js`][node-convenience]                                                               | The three-argument options overload preserves callback arity, receiver, result type, and async order. |
| `constants/immutability.ts`              | [`test-zlib-const.js`][node-const]                                                                                   | `codes`, its values, and `constants` values reject writes and keep their values.                      |
| `convenience/constructors.ts`            | [`test-zlib-deflate-constructors.js`][node-constructors], [`zlib.test.js`][bun-zlib-main]                            | Public constructors work with and without `new`, keep their names, and create matching instances.     |
| `convenience/factory-instances.ts`       | [`lib/zlib.js`][node-lib], [`zlib.test.js`][bun-zlib-main]                                                           | Deflate, Inflate, Gzip, Unzip, and Brotli factories return their matching public classes.             |
| `convenience/info-result.ts`             | [`test-zlib-convenience-methods.js`][node-convenience]                                                               | Sync compression and decompression with `info: true` return `{ buffer, engine }`.                     |
| `convenience/prototype-chains.ts`        | [`lib/zlib.js`][node-lib], [`zlib.test.js`][bun-zlib-main]                                                           | Constructor descriptors and Zlib/Brotli prototype parents match Node.                                 |
| `crc32/seed-validation.ts`               | [`test-zlib-crc32.js`][node-crc], [`test-zlib-negative-zero.js`][node-negative-zero]                                 | CRC seeds accept uint32 bounds, coerce `-0`, and reject wrong types and ranges.                       |
| `dictionary/brotli-roundtrip.ts`         | [`test-zlib-brotli-dictionary.js`][node-brotli-dictionary]                                                           | Brotli uses the supplied dictionary and fails without it.                                             |
| `dictionary/deflate-roundtrip.ts`        | [`test-zlib-dictionary.js`][node-dictionary]                                                                         | Deflate output requires the matching preset dictionary.                                               |
| `dictionary/invalid-type.ts`             | [`test-zlib-deflate-constructors.js`][node-constructors]                                                             | Deflate rejects non-buffer dictionary values with `ERR_INVALID_ARG_TYPE`.                             |
| `dictionary/source-types.ts`             | [`test-zlib-deflate-constructors.js`][node-constructors], [`test-zlib-brotli-dictionary.js`][node-brotli-dictionary] | Buffer, Uint8Array, DataView, and ArrayBuffer dictionaries keep their bytes.                          |
| `dictionary/stream-application.ts`       | [`test-zlib-dictionary.js`][node-dictionary]                                                                         | `createDeflate()` applies, rather than only accepts, its dictionary.                                  |
| `dictionary/zstd-roundtrip.ts`           | [`test-zlib-zstd-dictionary.js`][node-zstd-dictionary]                                                               | Zstd output requires the matching dictionary.                                                         |
| `gzip/trailing-null-bytes.ts`            | [`test-zlib-from-gzip-with-trailing-garbage.js`][node-trailing]                                                      | Sync and callback gunzip ignore fixed trailing NUL bytes after valid members.                         |
| `imports/legacy-constant-aliases.ts`     | [`lib/zlib.js`][node-lib]                                                                                            | Legacy direct constants equal `constants.*` and use non-enumerable, read-only descriptors.            |
| `inputs/shared-array-buffer.ts`          | [`lib/zlib.js`][node-lib], [`test-zlib-convenience-methods.js`][node-convenience]                                    | One-shot codecs accept `SharedArrayBuffer` and return a Buffer.                                       |
| `options/brotli-params-roundtrip.ts`     | [`test-zlib-brotli.js`][node-brotli], [`zlib_test.ts`][deno-test]                                                    | Fixed Brotli quality values round-trip semantically at min, middle, and max.                          |
| `options/chunk-size-validation.ts`       | [`test-zlib-deflate-constructors.js`][node-constructors]                                                             | `chunkSize` checks type, lower bound, NaN, and infinity.                                              |
| `options/flush-validation.ts`            | [`test-zlib-flush-flags.js`][node-flush-flags]                                                                       | `flush` and `finishFlush` accept valid flags and reject wrong types and ranges.                       |
| `options/level-strategy-validation.ts`   | [`test-zlib-deflate-constructors.js`][node-constructors], [`test-zlib-failed-init.js`][node-failed-init]             | `level`, `memLevel`, and `strategy` enforce type/range rules and map NaN to defaults.                 |
| `options/max-output-length.ts`           | [`test-zlib-maxOutputLength.js`][node-max-output], [`zlib_test.ts`][deno-test]                                       | Sync and callback Brotli enforce the output cap and return `ERR_BUFFER_TOO_LARGE`.                    |
| `options/validation-order.ts`            | [`lib/zlib.js`][node-lib]                                                                                            | Observable option getters run in Node's exact order.                                                  |
| `options/window-bits-zero.ts`            | [`test-zlib-zero-windowBits.js`][node-zero-window]                                                                   | Zero is valid for inflate/gunzip/unzip; nonzero 1..7 and 16 reject, 8/15 and defaults remain valid, wrong types reject, and raw codecs/compressors reject zero.                                   |
| `streams/bytes-written-trailing.ts`      | [`test-zlib-premature-end.js`][node-premature], [`bytesWritten.test.ts`][bun-bytes-written]                          | A decoder counts consumed compressed bytes, not fixed trailing input.                                 |
| `streams/close-state.ts`                 | [`test-zlib-close-after-write.js`][node-close-write], [`lib/zlib.js`][node-lib]                                      | `close()` is idempotent; `_closed` follows handle falsiness, while deferred destroy completion controls Transform closed state.              |
| `streams/codec-roundtrips.ts`            | [`test-zlib-create-raw.js`][node-create-raw], [`test-zlib-brotli-from-string.js`][node-brotli-string]                | Deflate, raw, gzip, unzip, and Brotli factories each complete a stream round-trip.                    |
| `streams/error-close.ts`                 | [`test-zlib-close-after-error.js`][node-close-error]                                                                 | Decode errors observe completed closed state before error/close delivery; generic deferred destroy preserves replacement errors and event order.                             |
| `streams/flush-order.ts`                 | [`test-zlib-flush-write-sync-interleaved.js`][node-flush-order]                                                      | Write and flush callbacks keep enqueue order through end.                                             |
| `streams/reset-before-write.ts`          | [`test-zlib-reset-before-write.js`][node-reset-before]                                                               | `reset()` and `params()` both work before the first write.                                            |
| `unzip/one-byte-members.ts`              | [`test-zlib-unzip-one-byte-chunks.js`][node-one-byte]                                                                | Streaming unzip detects concatenated gzip members across one-byte writes.                             |
| `validation/params-arguments.ts`         | [`test-zlib-deflate-constructors.js`][node-constructors]                                                             | `params()` validates level before strategy and reports exact error classes and codes.                 |
| `validation/reject-trailing-garbage.ts`  | [`test-zlib-reject-garbage-after-end.js`][node-reject-garbage]                                                       | The new Node 26 boolean option rejects a second deflate stream and validates its type.                |
| `validation/truncated-finish-flush.ts`   | [`test-zlib-truncated.js`][node-truncated]                                                                           | Default decompression rejects truncation while `Z_SYNC_FLUSH` returns a valid prefix.                 |

## Complete fixture map

Each name below is relative to this directory. The added-contract table gives
the exact source for every new fixture; the original fixtures use the same
locked Node, Deno, and Bun source families.

| Area           | Fixtures                                                                                                                                                                                                                                               | Contract group                                                                         |
| -------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------- |
| `async/`       | `callback-one-shot.ts`, `error-callback-shape.ts`, `options-overload-and-callback.ts`, `promisify-gzip.ts`, `promisify-one-shot.ts`                                                                                                                    | Callback and promisify one-shot behavior.                                              |
| `brotli/`      | `roundtrip-ascii.ts`, `roundtrip-empty.ts`, `roundtrip-large.ts`                                                                                                                                                                                       | Semantic sync Brotli round-trips.                                                      |
| `constants/`   | `brotli.ts`, `codes.ts`, `compression-levels.ts`, `flush-values.ts`, `immutability.ts`, `modern-version-table.ts`, `return-codes.ts`, `strategy.ts`                                                                                                    | Public constant values, reverse codes, modern entries, and mutability.                 |
| `convenience/` | `callback-fn-shapes.ts`, `class-shapes.ts`, `constructors.ts`, `factory-instances.ts`, `factory-shapes.ts`, `info-result.ts`, `prototype-chains.ts`                                                                                                    | Function, class, factory, instance, prototype, and info shapes.                        |
| `crc32/`       | `basic.ts`, `binary-vectors.ts`, `seed-chaining.ts`, `seed-validation.ts`, `string-input.ts`                                                                                                                                                           | Reference vectors, input forms, chaining, and seed rules.                              |
| `deflate/`     | `roundtrip-ascii.ts`, `roundtrip-binary.ts`, `roundtrip-empty.ts`, `zlib-format-header.ts`                                                                                                                                                             | Zlib-wrapped deflate semantics and framing.                                            |
| `dictionary/`  | `brotli-roundtrip.ts`, `deflate-roundtrip.ts`, `invalid-type.ts`, `source-types.ts`, `stream-application.ts`, `zstd-roundtrip.ts`                                                                                                                      | Dictionary application, source types, and validation.                                  |
| `errors/`      | `brotli-invalid.ts`, `gunzip-bad-magic.ts`, `gunzip-truncated.ts`, `inflate-raw-format.ts`, `unzip-garbage.ts`                                                                                                                                         | Fixed invalid and truncated inputs.                                                    |
| `gzip/`        | `magic-bytes.ts`, `multi-member.ts`, `roundtrip-ascii.ts`, `roundtrip-binary.ts`, `roundtrip-empty.ts`, `roundtrip-large.ts`, `roundtrip-utf8.ts`, `trailing-null-bytes.ts`                                                                            | Gzip framing, inputs, members, and allowed trailing NUL bytes.                         |
| `imports/`     | `class-exports.ts`, `legacy-constant-aliases.ts`, `named-import.ts`, `namespace-import.ts`, `prefixless-import.ts`                                                                                                                                     | ESM/module shape and legacy aliases.                                                   |
| `inputs/`      | `hex-encoding.ts`, `shared-array-buffer.ts`, `string-direct.ts`, `uint8array.ts`                                                                                                                                                                       | Fixed binary and string input forms.                                                   |
| `options/`     | `brotli-params-roundtrip.ts`, `chunk-size-validation.ts`, `flush-validation.ts`, `level-strategy-validation.ts`, `max-output-length.ts`, `validation-order.ts`, `window-bits-zero.ts`                                                                  | Option semantics, validation, defaults, getters, and output caps.                      |
| `raw/`         | `no-zlib-header.ts`, `roundtrip-ascii.ts`, `roundtrip-empty.ts`                                                                                                                                                                                        | Raw deflate framing and round-trips.                                                   |
| `streams/`     | `bytes-written-trailing.ts`, `close-destroy-method-values.ts`, `close-state.ts`, `codec-roundtrips.ts`, `create-gzip-roundtrip.ts`, `error-close.ts`, `factory-typeofs.ts`, `flush-order.ts`, `instance-methods-and-bytes.ts`, `reset-before-write.ts` | Zlib stream methods, state, codecs, ordering, cleanup, and byte counts.                |
| `unzip/`       | `auto-detect-deflate.ts`, `auto-detect-gzip.ts`, `one-byte-members.ts`                                                                                                                                                                                 | Format detection and member boundaries.                                                |
| `validation/`  | `callback-required.ts`, `one-shot-buffer-sources.ts`, `one-shot-invalid-data.ts`, `one-shot-return-buffers.ts`, `params-arguments.ts`, `reject-trailing-garbage.ts`, `truncated-finish-flush.ts`                                                       | Trust-boundary types, callbacks, return types, params, trailing input, and truncation. |
| `zstd/`        | `one-shot-roundtrip.ts`, `stream-roundtrip.ts`                                                                                                                                                                                                         | Zstd sync, callback, promisify, constructor, and stream behavior.                      |

## Audit exclusions and stop rule

The audit read every Node 26.5.0 zlib-named test plus the primary
implementation. It stopped after the second pass found no other contract that
was deterministic, portable, non-redundant, and safe for this granular suite.

- We excluded 16 GiB, `kMaxLength`, OOM, memory-pressure, heapdump, fuzz,
  random-byte, GC, weak-reference, and race tests.
- We excluded fixture-file and fs-pipeline tests when a fixed in-memory fixture
  already covers the zlib contract.
- We excluded exact Brotli, Zstd, gzip, and flush bytes because backend and
  library versions may change them. The suite compares decoded content, error
  shape, or state instead.
- We excluded generic Transform behavior already owned by the stream suite:
  object writes, pipe teardown, write-after-end, write-after-close, and drain
  pressure. The retained lifecycle cases probe zlib-specific state or ordering.
- We excluded snapshot, worker, async-hooks provider, and ALS propagation tests.
- We excluded private handle calls, internal inheritance hacks, and private
  write-state memory tests. They test Node, Deno, or Bun internals rather than
  Perry's public `node:zlib` contract.
- We used no files, workers, signals, sleeps, timers, random data, large inputs,
  forced GC, or backend-specific compressed fixtures.

[node-tree]: https://github.com/nodejs/node/tree/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb
[node-lib]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/lib/zlib.js
[node-const]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-const.js
[node-convenience]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-convenience-methods.js
[node-constructors]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-deflate-constructors.js
[node-crc]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-crc32.js
[node-negative-zero]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-negative-zero.js
[node-dictionary]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-dictionary.js
[node-brotli-dictionary]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-brotli-dictionary.js
[node-zstd-dictionary]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-zstd-dictionary.js
[node-trailing]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-from-gzip-with-trailing-garbage.js
[node-brotli]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-brotli.js
[node-flush-flags]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-flush-flags.js
[node-failed-init]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-failed-init.js
[node-max-output]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-maxOutputLength.js
[node-zero-window]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-zero-windowBits.js
[node-premature]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-premature-end.js
[node-close-write]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-close-after-write.js
[node-create-raw]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-create-raw.js
[node-brotli-string]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-brotli-from-string.js
[node-close-error]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-close-after-error.js
[node-flush-order]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-flush-write-sync-interleaved.js
[node-reset-before]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-reset-before-write.js
[node-one-byte]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-unzip-one-byte-chunks.js
[node-reject-garbage]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-reject-garbage-after-end.js
[node-truncated]: https://github.com/nodejs/node/blob/bebd1b8d92bf4cc917844d6335ed1ecf9c2a75fb/test/parallel/test-zlib-truncated.js
[deno-tree]: https://github.com/denoland/deno/tree/34c46613cbe20450b74c0e8d4f0fd8f6f781d807
[deno-test]: https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/tests/unit_node/zlib_test.ts
[deno-lib]: https://github.com/denoland/deno/blob/34c46613cbe20450b74c0e8d4f0fd8f6f781d807/ext/node/polyfills/zlib.js
[bun-tree]: https://github.com/oven-sh/bun/tree/43d60f69c95a9f31591165816ced29b83e94673e
[bun-lib]: https://github.com/oven-sh/bun/blob/43d60f69c95a9f31591165816ced29b83e94673e/src/js/node/zlib.ts
[bun-zlib-tests]: https://github.com/oven-sh/bun/tree/43d60f69c95a9f31591165816ced29b83e94673e/test/js/node/zlib
[bun-zlib-main]: https://github.com/oven-sh/bun/blob/43d60f69c95a9f31591165816ced29b83e94673e/test/js/node/zlib/zlib.test.js
[bun-bytes-written]: https://github.com/oven-sh/bun/blob/43d60f69c95a9f31591165816ced29b83e94673e/test/js/node/zlib/bytesWritten.test.ts

## Descriptor and write policy

`constants/descriptor-write-policy.ts` checks the pinned oracle's complete
legacy-alias descriptor surface, read-only constants and module exports, the
frozen bidirectional `codes` object, and rejection without value changes.
Legacy aliases remain non-enumerable; ordinary function exports remain mutable
and can be restored after an override. Descriptor values share the same
`constants` object as ordinary reads. This supplements `constants/immutability`
and `imports/legacy-constant-aliases` without changing those fixtures.

## Synchronous decoder completion

`inflateSync`, `gunzipSync`, and `inflateRawSync` retain completion options on
both direct imports and captured exports for their bare-Buffer paths.
The genuine `inflateSync` `info: true` owner path retains its existing behavior;
combining info results with completion policy is outside this decoder change. `rejectGarbageAfterEnd` accepts only
booleans or `undefined`; explicit `true` rejects bytes after the first stream
with `TypeError [ERR_TRAILING_JUNK_AFTER_STREAM_END]`, including another gzip
member. Defaults still accept concatenated gzip members and gzip zero padding.
An explicit permissive `finishFlush` returns the available prefix only for
exhausted-input EOF, never corruption or missing/wrong dictionary errors.

The validation fixtures cover direct/captured calls, defaults and error controls.
Truncated decoding uses identical fixed Node 26.12.1 wire inputs, not halves of
independently compressed streams with different Huffman representations; fixed
miniz prefixes also cover exhausted input producing an empty Buffer. This tests
decoder completion without requiring identical compressor output.

### Synchronous DEFLATE info results

`deflateSync` and `inflateSync` return an enumerable `{ buffer, engine }`
record when `options.info` is truthy. The genuine `Deflate` or `Inflate` owner
processes the input through its attached codec; `bytesWritten` reports consumed
input, and releasing its native handle does not destroy or close the Transform.
False/default info retains the bare Buffer and existing one-shot backend.
The public fixture covers named, namespace and captured calls, truthiness,
option-getter order, malformed input, and view-bounded input accounting.
