# Rust dependency audit and initial cleanup — 14 September 2026

**Implemented in this PR:** remove 22 unused member dependency declarations and the unused `similar` workspace template, including stdlib's unused Clap and Tokio cron scheduler dependencies. The lockfile loses 15 external package versions, with no added packages, version upgrades or checksum changes to retained packages. Existing Tokio usage and all library implementations remain in place. ZIP feature trimming, Governor changes, version alignment and replacement projects below are recommendations for separate changes.

The dated census, decisions and graph experiments preserve the **pre-cleanup baseline** so the evidence remains independently reviewable. The removed declarations are listed below; the dependency decision table describes the baseline and is not a census of the post-cleanup tree.

The best next steps are **remove unused declarations, narrow features, and selectively own small implementations**. The strongest new implementation candidates are NanoID generation, the remaining legacy rate limiter, and the deliberately small PDF writer. Keep the large standards, compiler, protocol, cryptographic and platform libraries. Tokio's removal verdict remains with the separate session.

This assessment is pinned to `main` commit `4945fc1f7498debc76e9f861d7cf1517da5e67c9` (Perry 0.5.1565). The cleanup branches from that same baseline. `git fetch origin main` and `git pull --ff-only origin main` completed in the isolated `codex/rust-dependency-audit` worktree; the original checkout's concurrent work is preserved.

**Scope and evidence**

The workspace has **83 members, 195 distinct direct external package names, 990 registry package versions representing 863 names, and 13 additional vendored package versions**. Exactly 100 registry names occur at multiple versions. These are dependency identities, not the number of libraries linked into an executable. Perex is a registry dependency maintained by Perry itself.

Every direct library has an individual verdict, rationale, owner list, version list and example source/manifest link in the [195-library decision table](audits/rust-dependency-decisions-2026-09-14.md). The [complete census](audits/rust-dependencies-2026-09-14.json) also records dependency kind, optional/default features, target predicates, renamed imports, lexical usage, every external lock package and its immediate parents. Transitive libraries are assessed through their owning dependency, feature selection and shared reachability; this is not an independent algorithm or vulnerability audit of all 1,003 external package identities.

All tracked Cargo manifests were enumerated, including 22 additional first-party manifests outside the workspace: benchmark/reference implementations, archived benchmark prototypes, the LLVM spike, fuzzing and provider fixtures. Their external declarations introduce no additional library names beyond the census. Their independent lockfiles are outside the workspace graph measurements; changing a reference benchmark to use Perry's implementation would also invalidate its independence. Vendored Windows manifests are recorded separately.

The initial assessment uses source inspection, locked Cargo metadata, 21 selected Cargo trees, six independent manifest counterfactuals, and primary upstream documentation. The worktree selects `cargo 1.100.0-nightly (514c56dd7 2026-08-19)`. The baseline experiments did not compile Rust or measure runtime behavior/performance; implementation validation is recorded separately in the PR. No size or speed improvements are claimed. `cargo tree -e normal,build` approximates the build graph but is not an exact list of compilation units or linked code; Cargo documents this distinction in its [tree reference](https://doc.rust-lang.org/cargo/commands/cargo-tree.html).

**Recommended order**

| Priority | Work | Why it is worth doing | Acceptance condition |
|---|---|---|---|
| Done in this PR | Remove unused `tokio-cron-scheduler`, stdlib `clap`, compose `atty`, and other stale edges below | Already implemented functionality or no caller; 15 lock package versions disappear | Validation results are recorded in the PR |
| 1 | Limit CLI ZIP support to the archive methods its supported inputs require | Largest demonstrated compiler graph reduction in this audit | Update ZIP, IPA and HAP fixtures still read/write correctly, including failure cases |
| 1 | Align compose `dialoguer` with workspace 0.12 | Removes an avoidable old terminal stack | Installer prompt behavior and noninteractive behavior remain correct |
| 2 | Replace owned `lazy_static` / `once_cell` uses with `std` | Small maintenance simplification; toolchain already supports it | Initialization, concurrency and GC scanner registration semantics preserved |
| 2 | Trim Governor features, then evaluate owning its legacy limiter | Most npm-compatible state logic is already local | Direct GCRA and keyed fixed-window behavior both retain their contracts |
| 2 | Consolidate NanoID generation on the existing RNG/helper path | Very small external API surface; custom alphabet is already local | Secure randomness, unbiased selection, lengths and alphabets preserved |
| 2 | Spike a minimal PDF writer | Five small APIs currently carry a font/PDF-processing graph | Structural, text-encoding and rendering parity plus worthwhile measured savings |
| 3 | Improve feature boundaries: TUI, updater, SQL backends, image formats, CLI-only helpers | Better fit between requested functionality and selected libraries | Relevant APIs enable their features, unrelated programs omit them, dynamic use remains covered |
| 3 | Align TLS providers and compatible duplicated library generations | Potential native build/link savings; already has compatibility constraints | Cross-adapter TLS/key/certificate tests and supported-platform builds |

These priorities are engineering judgments, not measured benefit-to-effort ratios. The graph experiments make the first few tasks concrete; replacement spikes must still establish performance and maintenance value.

**Measured graph counterfactuals**

Each experiment started from the same baseline and changed only the listed manifest declarations. Cargo resolved offline; the original manifests and lockfile were restored byte-for-byte after each experiment. Counts are unique external package versions in the Windows x64 normal/build tree, including build dependencies. These changes were not compiled. Exact edits and package differences are in the [experiment record](audits/rust-dependency-experiments-2026-09-14.json).

| Independent experiment | Selected package / features | Before → after | Interpretation |
|---|---|---:|---|
| ZIP: disable defaults, enable `deflate-flate2-zlib-rs` | `perry`, default | 352 → 335 | 17 fewer packages, retaining ZIP and Deflate; drops unused codec/encryption branches in this configuration |
| Remove `tokio-cron-scheduler` declaration and feature activation | `perry-stdlib`, default | 556 → 543 | 13 fewer, including its separate cron parser and timezone/derive stack; Tokio itself remains |
| Remove stdlib's `clap` declaration | `perry-stdlib --no-default-features` | 149 → 134 | 15 fewer; Commander is already local code |
| Remove compose's `atty` declaration | `perry-container-compose` | 98 → 96 | Removes `atty` and this graph's `winapi` |
| Inherit workspace `dialoguer` 0.12 | `perry-container-compose` | 98 → 94 | Five old packages disappear and one new Dialoguer version appears: net four fewer |
| Governor: disable defaults, retain `std`, at both declaring edges | `perry-ext-ratelimit` | 37 → 27 | Ten fewer, principally Quanta and its RNG generation; still uses Governor |

These savings must not be added together across different packages/configurations. Shared dependency and feature unification changes the result of combined edits. In particular, removing an edge from minimal stdlib can have no package-count effect when the CLI or another selected crate independently needs it.

**What makes sense to own**

| Library | Assessment | Scope and validation | Expected implementation effort |
|---|---|---|---|
| `lazy_static`, `once_cell` | Yes: use standard library equivalents | Owned `once_cell` sites use `sync::Lazy`; migrate to `LazyLock`, and use `OnceLock` for explicit initialization. Preserve locking, panic behavior and root scanners | Small, mechanical migration with focused concurrency review |
| `nanoid` | Yes: good small replacement candidate | Only default/sized generation needs the crate; custom alphabet is already implemented in both adapters. Share one helper using a maintained cryptographic RNG, with unbiased sampling and compatible length/alphabet behavior | Small implementation and boundary tests |
| `governor` | Plausible after feature trimming | Keyed `RateLimiterMemory` already uses local fixed-window state. Legacy `js_ratelimit_new/check/remaining` still uses Governor's direct GCRA; replacing it with the keyed fixed window would change behavior. Test refill, bursts, multi-point requests, clock boundaries and concurrent callers | Small-to-medium state-machine project |
| `printpdf` | Best focused subsystem spike | Current API: create, add text, add line, new page, save. Built-in Helvetica, explicit page coordinates; HTML is already disabled. A small object/xref/content-stream writer could avoid the font-processing stack. Preserve escaping, non-ASCII behavior, page finalization and I/O failure handling; verify multiple PDF readers and rendered output | Medium project if scope remains exactly this API |
| `lru` | Possible, lower priority | An indexed linked recency list plus lookup map can fit the contract. Existing wrapper additionally handles TTL, arbitrary JS keys/values and mutable GC roots. Preserve O(1) operations, eviction order and moving-GC correctness; avoid a linear scan replacement | Medium; savings need measurement |
| `env_logger` | Possible, lower priority | Only `main` installs it. A compact `log::Log` implementation is feasible if it preserves required `RUST_LOG` filtering, formatting and thread-safe writes. Keep the shared `log` facade | Small-to-medium depending on filter compatibility |
| `hex` | Technically easy, little current payoff | A shared encoding/decoding helper is small. Preserve case, malformed input behavior and allocation patterns. It remains widely used transitively, so this is mainly code ownership | Small, but behind graph-reducing work |
| `dashmap` | Investigate only if profiles justify it | Handles cross FFI, async work, retirement/quarantine and GC scanners. Replacing maps with mutexes or slabs requires a lifetime/reentrancy design and contention measurements. The two handle registries have distinct domains | Substantial review; no blanket replacement recommendation |

Rust's standard `LazyLock` has been stable since 1.80 and `OnceLock` since 1.70; the [Rust 1.80 announcement](https://blog.rust-lang.org/2024/07/25/Rust-1.80.0/) and [OnceLock documentation](https://doc.rust-lang.org/std/sync/struct.OnceLock.html) describe their initialization contracts. Terminal detection is also available as [`std::io::IsTerminal`](https://doc.rust-lang.org/std/io/trait.IsTerminal.html), though the current unused `atty` edge needs deletion rather than a replacement call.

For context, a frozen cross-target lock-graph reachability calculation removes **22 package identities for PrintPDF, six for Governor, one for NanoID, one for LRU, four for env_logger, and zero for lazy_static/once_cell** when all direct workspace edges to that name are removed. This is a structural calculation, not a rebuilt graph: third-party edges/features remain fixed, and it includes optional/platform branches. It helps avoid attributing shared dependencies to every candidate. In particular, NanoID's `rand 0.9` remains needed by BSON/MongoDB, WebSocket, image and PDF stacks. Taking ownership of one helper does not remove that RNG generation workspace-wide.

Relevant source: [NanoID](../crates/perry-ext-nanoid/src/lib.rs), [rate limiting](../crates/perry-ext-ratelimit/src/lib.rs), [PDF writer](../crates/perry-ext-pdf/src/lib.rs), [LRU wrapper](../crates/perry-ext-lru-cache/src/lib.rs), [FFI handles](../crates/perry-ffi/src/handle.rs). Governor's defaults include `dashmap`, `jitter` and `quanta`; its [feature documentation](https://docs.rs/crate/governor/0.10.4/features) supports trimming those independently of the core limiter. Its [state model](https://docs.rs/governor/0.10.4/governor/state/index.html) uses GCRA.

**Declarations removed in this PR**

Source searches covered each owner's Rust source, tests, examples, benches and build script, ignoring whole-line comments. The cleanup removes the declarations below and the scheduler's obsolete `dep:tokio-cron-scheduler` feature activation. Lexical absence alone is not a compilation proof; implementation checks and platform limits are recorded in the PR.

| Owner | No lexical references found | Notes |
|---|---|---|
| `perry-stdlib` | `anyhow`, `thiserror`, `clap`, `itoa`, `ryu`, `tokio-cron-scheduler` | Commander and the cron timer queue are already owned implementations |
| `perry-container-compose` | `anyhow`, `atty`, `dashmap`, `dotenvy` | Installer uses `console::Term`; errors use the crate's `thiserror` type |
| `perry-transform` | `anyhow`, `thiserror` | Remove declarations after target/test checks |
| `perry-runtime` | `anyhow`, `thiserror` | Not a proposal to remove live compiler `anyhow` uses |
| `perry-codegen`, `perry-hir`, `perry-parser` | `thiserror` | `anyhow` is extensively used and stays |
| `perry-updater` | `serde_json` | Keep its actual Serde, signature and hashing dependencies |
| `perry-ext-fastify` | `lazy_static` | Other extension users still use it |
| `perry-ui-android` | `itoa`, `ryu`, `serde_json` | Target-specific validation needed |
| Workspace template | `similar` | No member inherits it; deletion has zero graph effect |

These cases require a different verdict:

- **Keep `windows-core`** in Windows UI. The `windows::core::implement` macro expands to absolute `::windows_core` paths. The manifest documents the build failure if the direct edge is removed.
- **Keep `base64` and `libc` in WinUI**. Its `#[path]` modules compile the shared Windows backend's [system FFI](../crates/perry-ui-windows/src/ffi/system.rs), including Base64 encoding and allocation cleanup, in the WinUI crate. The census scans files inside each package and does not follow shared source paths; Windows compilation confirmed these declarations are required.
- **Keep/review feature propagation for `icu_calendar` and `icu_time`**. Runtime enables their `compiled_data` under `intl-datetime`; re-exports hide lexical use. Removing an edge could change selected data even without an import.
- **Review only the unused stdlib version/feature edges for `sha3` and `spki`**. Stdlib's `sha3 0.12` alias has no lexical use, while SHAKE uses `shake` and KMAC uses `sha3_010`. `spki` is directly used by node-forge and can activate PEM/alloc features elsewhere. Preserve the live crypto algorithms and re-export feature contracts.

**Feature and version work with more potential than rewrites**

ZIP is a strong immediate target. The CLI reads update ZIPs and IPAs and writes Deflated HAP files. `zip = "8"` enables AES, Bzip2, Deflate64, LZMA/XZ, PPMd, Zstd and an additional Deflate encoder. The experiment retains `deflate-flate2-zlib-rs`; do not use a feature named `deflate` blindly, because that umbrella also enables Zopfli in the locked release. Verify the actual supported archive inputs before narrowing readers. The family feature layout is documented in [Zip 8's feature list](https://docs.rs/crate/zip/8.0.0/features); the precise 8.6.0 manifest and resolved features were inspected locally.

Image defaults are broad, including EXR, TIFF, QOI and AVIF-related dependencies. `perry-ext-sharp` actually calls automatic input detection, AVIF encoding, JPEG quality encoding and `fast_image_resize`; deleting those formats or the SIMD resizer would change supported behavior/performance. Establish an input-format contract before using `default-features = false`. `perry-doc-tests` is a separate opportunity to request only its fixture formats. Coordinate **every** `image` edge because features are additive.

`perry-stdlib` currently enables SQLx MySQL and PostgreSQL together on one optional dependency. Its separate bundled backend features can activate the individual `sqlx/mysql` and `sqlx/postgres` branches instead; the extension crates already select their individual backends. Check direct and external-wrapper builds, JSON/Chrono conversions and feature unification. Retain SQLx, MongoDB, Redis and SQLite implementations: protocol authentication, transactions, errors and cancellation are not small helper contracts.

The always-on runtime dependency `taffy` already disables default features and selects flexbox. The useful next step is a TUI feature boundary, including its dynamic/namespace exposure, rather than owning a flexbox engine. Likewise, stdlib always re-exports `perry-updater`; a dedicated updater gate could stop unrelated minimal stdlib builds from selecting its signature/manifest stack. That would require coordinating compiler feature detection and archive symbol retention, not merely marking the dependency optional.

TLS uses both providers **intentionally in code today**: HTTP explicitly selects Ring, while net/WebSocket and several bundled stdlib paths select AWS-LC. Some paths install a process-wide default. Unifying providers could reduce native build/link work, but requires changing these call sites together, preserving private-key/certificate algorithms, roots, TLS versions and any provider-specific capability. The default CLI graph already uses Ring alone; the dual-provider finding applies to broader stdlib/extensions and should not be attributed to every executable. Keep Rustls and the crypto libraries. Coordinate networking changes with the Tokio study.

Version alignment also has concrete boundaries:

- **Dialoguer 0.11/0.12**: a direct actionable duplicate; align compose, then trim unused prompt features.
- **SWC parser 29/32**: 29 is still pulled by `swc_ecma_transforms_base` and the bundler/transform stack, while direct parsing uses 32. Update compatible SWC families together or reduce unnecessary transform use; a parser rewrite is a major compiler project.
- **Crypto generations** (`digest`, `cipher`, `rand_core`, RSA/DER/SPKI and hash crates): direct aliases bridge real API generations. Upgrade compatible consumers together; forcing one version is not a valid fix.
- **Windows 0.35/0.62**: old XAML Islands/MapControl projections are deliberately isolated behind `windows-xaml`; no blind version unification. COM `windows-core` and WebView2 types must remain aligned. Vendored Reactor dependencies are already separately maintained upstream code, not unused workspace members.
- **GTK/GLib/Cairo generations**: multiple target-only bindings occur across GTK/WebKit and media/tray services. Align supported release families if those paths coexist; do not count them as default compiler payload.
- **`serde_yaml 0.9.34+deprecated`**: an upstream maintenance migration to investigate, with Compose syntax/merge/tag/diagnostic fixtures. Owning a YAML parser is not a good reduction strategy. Coordinate its `unsafe-libyaml` backend with the gated Bun YAML API.

**Libraries to keep**

Keep SWC, LLVM/Inkwell, Wasmi, Wasm encoding, cryptographic primitives and certificate formats, HTTP/TLS/WebSocket/DNS/database implementations, compression codecs, Unicode/IDNA/ICU/Temporal data, native UI/service bindings, and the decimal engine. The reason is the breadth of their actual Perry contract. Existing gates are usually the appropriate place to reduce cost. Replacing Inkwell's Rust API does not remove the linked LLVM implementation, and replacing a Rust OS wrapper does not remove the underlying operating-system dependency.

Also keep the small, performance-sensitive helpers unless an A/B says otherwise: `ahash` for duplicate-key hashing, `aho-corasick` for retained-source matching, `itoa`, `ryu-js`, `simdutf8`, `rayon` for compiler parallelism, `gimli` for unwind CFI, `object` for native object formats, and `stacker` for deep lowering/parsing. A single import can front a substantial algorithmic or correctness contract. `hex`, `base64`, `walkdir`, `which`, `tempfile`, `fslock`, `dirs`, `hostname`, `httpdate`, `qrcodegen`, `cron`, `semver` and EXIF parsing are individually assessed in the decision table; their modest implementation sizes alone do not establish a worthwhile replacement.

Perry already owns several of the requested replacements: Perex (`PerryTS/perex`), borrowed validators in `perry-validation`, Commander parsing, cron's timer queue, keyed rate limiting, and JS JSON hot paths. Preserve `regex` and `validator` as differential dev oracles. Removing those test edges does not optimize shipped behavior; `regex` may still be selected transitively by unrelated libraries. Preserve general Serde/JSON/TOML parsing for Rust-side configuration and interchange. Rust `semver` and npm `node-semver` also implement different range contracts.

**Selected graph baseline**

Each row selects just the named package with normal/build edges; these are not the combined package invocations used to distribute static archives. Platform UI extensions are separately selected. `minimal` means `--no-default-features`, not a claim that this is a representative application's detected feature set. Full commands and package identities are in the [21-graph baseline](audits/rust-dependency-build-graphs-2026-09-14.json).

| Selected configuration | Windows x64 | Linux x64 | macOS arm64 |
|---|---:|---:|---:|
| CLI default | 352 | 352 | 349 |
| CLI `--no-default-features --features dev-cli` | 350 | 350 | 347 |
| Runtime default | 218 | 216 | 206 |
| Runtime minimal | 108 | 106 | 96 |
| Stdlib default | 556 | 549 | 546 |
| Stdlib minimal | 149 | 146 | 137 |
| Compose default | 98 | 94 | 92 |

The slim CLI drops only two external package identities in these graphs. Its command features often hide commands while shared command/helper modules and dependencies stay enabled; a genuinely narrower compiler distribution would need more module/dependency separation. Runtime's default and minimal graphs contain no Tokio here, and minimal stdlib's graph also contains no Tokio. These are existing feature configurations, not a verdict about replacing Tokio in networked applications.

**How to turn a candidate into a decision**

Use a separate implementation branch for each candidate and pin the same compiler commit, target, profile and package set on both sides. For an archive-affecting change, build `perry-runtime-static` / `perry-stdlib-static` and the same relevant extension set; rebuilding only the rlib crates does not update shipped archives. Pin `PERRY_RUNTIME_DIR`, verify archive provenance, and preserve the shared-library feature/codegen-unit configuration used for linking. The existing repository warning about stale `.a` files applies directly here.

Measure clean and incremental build time, archive/distribution sizes, final stripped program bytes, startup time/RSS and the candidate's relevant throughput/latency. Include a minimal program, a JSON workload, a TLS HTTP/WebSocket workload, a database workload and the affected specialized API (PDF/image/TUI as appropriate). Use `--report-size` for attribution where useful, while checking final files directly. Keep measurements isolated from concurrent sessions.

For semantics, use the affected Rust unit tests and existing Node parity fixtures; test malformed/boundary inputs, cancellation/close behavior, and both bundled and external adapter routes. Run runtime tests single-threaded as required by the repository. FFI/handle changes additionally require moving-GC and cross-thread lifetime coverage. PDF replacements need structural parsing plus visual/text validation. Crypto/provider changes need known-answer and certificate interoperability tests. A smaller dependency count alone does not qualify a replacement for landing.

The read-only census is reproducible with:

```text
python scripts/rust_dependency_inventory.py --output current-rust-dependency-inventory.json
cargo tree --locked -p perry -e normal,build --target x86_64-pc-windows-msvc
cargo tree --locked --workspace --target all -d
```

The first command inventories the current checkout and may download locked sources to Cargo's cache, but does not compile or modify the lockfile. Run it at the pinned baseline commit to reproduce the dated census; use a different output path after cleanup. Lexical use counts are deliberately review aids; the shared-source, macro and feature-only exceptions above demonstrate why automated deletion based on them would be unsound.
