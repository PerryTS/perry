# Dependency cleanup and pull-on-use boundaries

Baseline: `25ef4637f5` (before this PR). No version bump.

This change replaces the narrow dependency surfaces identified in the audit:

- `env_logger`: CLI logger over the existing `log` facade, with module/level
  selection and Perex message filtering through `RUST_LOG`.
- `chrono`: CLI UTC formatting and the certificate parser's existing
  `DateTime::unix_duration()` conversion. No new time-zone implementation.
- `jsonwebtoken`: shared Apple ES256 token framing; P-256 signing and PKCS#8
  parsing remain in RustCrypto. Both previous Apple-token callers share it.
- `uuid`: only v4 and monotonic v7, with OS entropy and independent opt-in
  features. v1/v3/v5 and their hash dependencies are no longer selected.
- `lazy_static` / `once_cell`: Perry's direct uses move to std `LazyLock`.
  Tokio's non-initializing activity probe keeps a separate initialization flag.
- `hex` / `base64`: dependency-free strict codec leaf crates. Existing permissive
  Node Buffer codecs remain distinct. Tests compare base64 against the previous
  0.23 implementation, including malformed inputs and optional source-map padding.
- `resolv-conf`: private nameserver parser in the DNS module. Search/options
  directives are ignored because this consumer uses only server addresses.
- `indicatif`: CLI progress display with terminal detection, bounded redraws,
  byte/rate/ETA output and an on-demand ticker for unknown-length downloads.
- `dotenvy`: publish's environment loader, with quoting, multiline values,
  interpolation and preservation of existing environment variables. The previous
  crate is retained only as a test oracle.

## Selection happens before linking

`perry-cli-support` has no default features or unconditional dependencies. Its
UTC, dotenv, logging, progress and Apple JWT modules are independently selected.
It is never a runtime dependency. `perry` selects the helpers its core command
modules use; `publish-cli` and `mobile-cli` select `apple-api`, which selects the
JWT module and P-256. A dev CLI does not compile that signing stack. Commands
that remain in the old shared CLI module structure report a clear feature error
if they reach Apple signing in a build without `apple-api`.

`perry-uuid` also defaults to no features/dependencies. `perry-stdlib/crypto`
selects v4+v7; `bundled-nodemailer` and `perry-ext-nodemailer` select v4. Minimal
stdlib builds do not pull this crate. The stdlib's always-on worker stack-size
helper was moved out of the optional async bridge so an empty-feature build
works without enabling async support.

The compiler/runtime source fingerprints include the new helper sources, so
editing a helper invalidates cached archives even without a version bump.

The hex/base64 crates contain only codecs and no initialization or retained FFI
entry points. Callers retain their existing optional dependency boundaries:
for example, runtime hex remains behind `node-api-host`, and stdlib's direct
codec edges follow crypto/TLS/fetch. Base64 is still a core runtime dependency
because core `atob`/`btoa`, source maps and secret-key dispatch call it. This PR
does not claim method-level feature elimination for those existing core APIs.
Likewise, minimal stdlib still reaches hex through its updater dependency.

The ZIP reader/writer defaults to Stored and Deflate, the formats used for Perry
release archives, IPA extraction and HAP generation. Builds requiring uncommon
third-party formats can select `perry/extended-zip`. This adds AES, bzip2,
deflate64, Zopfli, LZMA, PPMd, zstd, xz and time support. `deflate-flate2` remains
the backend-neutral feature: selecting `zip/deflate` would change the shared
`node:zlib` backend (#10810).

## Measured graph changes

Host: `aarch64-apple-darwin`. Each package was queried separately with
`cargo tree --locked --offline --edges normal,build --prefix none --format '{p}'`.
Counts deduplicate **package name + version** and include workspace crates.
These are selected build graphs, not the union recorded in Cargo.lock, and not
measurements of executable bytes or build-time improvements.

| Selected graph | Before | After | Net reduction |
| --- | ---: | ---: | ---: |
| Default `perry` | 339 | 287 | 52 |
| `perry --no-default-features --features dev-cli` | 331 | 255 | 76 |
| Default `perry-runtime` | 214 | 212 | 2 |
| Default `perry-stdlib` | 398 | 393 | 5 |
| `perry-stdlib --no-default-features` | 134 | 118 | 16 |

Third-party users retain some packages in other graphs: e.g. base64, once_cell,
lazy_static, regex, and MongoDB's UUID. Test oracles are dev-dependencies only.
No cryptographic primitives, Unicode data, parsers, concurrent handle maps,
float formatting algorithms or Aho-Corasick matching were replaced.

## Verification

`python3 scripts/check_lean_dependencies.py --offline` checks selected Cargo
graphs, both capability-absent and capability-present cases, separately to avoid
workspace feature unification masking errors. The check job runs this script
without `--offline`, together with helper tests and minimal stdlib/dev CLI checks.
An empty-feature CLI-support or UUID crate must have no dependencies; the codec
crates must have no normal/build dependencies. A deliberately planted prohibited
edge verifies the checker rejects it.

Helper tests cover canonical and malformed base64 against the previous library,
hex vectors, Gregorian boundaries, ES256 signature verification, UUID version/
variant bits, same-millisecond order, clock rollback and counter overflow,
logging filters, dotenv grammar, and hidden/no-color progress. Runtime DNS tests
cover comments, IPv4/IPv6, scoped addresses and invalid inputs.

Local validation: full runtime unit suite **4,438 passed, 4 ignored**; helper
unit tests and Clippy with warnings denied passed; full compiler/affected
extensions, dev CLI and empty-feature stdlib checked successfully. Repository
formatting, architecture, GC-root inventory, address inventory, test registration,
Node-version consistency and file-size gates passed.

The stdlib crypto subset has **22 passing and 2 failing tests**, identical on a
fresh build of `25ef4637f5` with the same features/profile and serial execution:
`native_dispatch_pbkdf2_value_form_fires_callback` and
`native_dispatch_random_bytes_value_form_fires_callback`. They expect callback-timer
completion and also fail without this cleanup. They were not disabled or changed.
