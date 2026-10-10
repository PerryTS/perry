# CPU cost of unpacking tarballs

The combined change reduces cold upm instructions by **10.54%**, locked median wall time by **15.97%**, and cycles by **14.34%**. Large-tarball worker instructions fall **30.3–30.5%** and cycles **24.5–24.7%**. Cold now uses 1.10× Node's instructions; median wall is 0.437 s against Node's 0.511 s. The full policy program set produces exactly Node's output.

**Architecture:** replace the Intel-gated wide Hash/HMAC provider with one CPU-dispatched context, and remove the separate one-shot inflate pipeline by sharing the existing streaming decoder. No Perry cache, latch, side table, package rule or upm edit was added.

Production commits are `cf2635e62` (wide digest) and `d839e45a0` (shared inflate). Baseline is fresh main `8ba3b4fa876b1e0dc03f6de84611108577910d37`. Performance reference is Node 24.9.0; repository parity uses 26.5.1. Default auto-optimization remains enabled. Source-stamped auto targets are separate per arm and built serially, with at most the compiler target plus one auto target present. Only tsc uses `PERRY_LL_RS4GC_MAX_INSTRS=2097152` in both arms: default main fails its LLVM IR guard at 1,615,932 against 1,572,864. `perrymaster` does not resolve; drivers/recipe come from the preserved coldextract/coldwall lanes, and shared buffer/worker hashes match `/root/shared/perf-drivers`.

The micro copies all 46 upm modules verbatim and runs its worker on the 8,075,816-byte rolldown tarball and all 22 pinned tarballs. Every worker file's bytes and mode match Node. Input messages own 64 KiB buffers. Preliminary subarray/backing-store probes and n=1 prechecks are excluded. Node micro totals include TypeScript stripping and worker startup; the recorded worker-ready noop permits an approximate subtraction.

Instructions: five interleaved runs, CPUs 0–55, `setarch -R`, off lock. Cycles/wall: CPUs 56–63 under `/root/MEASURE.lock`, after our builds finish; seven cold/lock runs and five micro runs. Tables show medians. Raw rows, min/max and MAD accompany the bundle; GC counts are separate diagnostic replays. Offinst timing is informational off-lock. The instruction noise floor is the larger of repeated-baseline median drift and within-arm MAD. Cold/lock/offinst floors are 1.106% / 0.284% / 0.0166%. Cold wall ranges are 0.445–0.536 / 0.417–0.474 / 0.496–0.534 s, with MAD 16.0 / 11.1 / 11.5 ms. Cold cycle MAD is 4.9% / 2.8% / 2.3%; timing on this shared SMT host is variable.

All triples are baseline / combined fix / Node. RSS is MiB; full collections are separate diagnostic medians.

| upm | Instructions G | Fix delta | Cycles G | Wall s | User s | RSS MiB | Full GC |
| --- | --- | --- | --- | --- | --- | --- | --- |
| cold | 7.338795 / 6.565431 / 5.959338 | -10.54% | 2.920 / 2.501 / 3.253 | 0.520 / 0.437 / 0.511 | 0.773 / 0.684 / 0.883 | 164.5 / 167.6 / 248.2 | 6 / 6 / 4 |
| lock | 2.363239 / 2.368430 / 3.596068 | +0.22% | 1.017 / 1.028 / 2.076 | 0.284 / 0.287 / 0.334 | 0.283 / 0.279 / 0.554 | 103.2 / 104.1 / 179.1 | 0 / 0 / 3 |
| offinst | 1.551410 / 1.551696 / 1.938745 | +0.02% | — | 0.216 / 0.198 / 0.275 | 0.181 / 0.234 / 0.442 | 77.0 / 78.0 / 131.5 | 0 / 0 / 1 |

| Program | Instructions G | Fix delta | Noise floor | RSS MiB | Full GC |
| --- | --- | --- | --- | --- | --- |
| hello | 0.000232 / 0.000232 / 0.303996 | +0.0875% | 0.0060% | 1.6 / 1.6 / 63.0 | 0 / 0 / 0 |
| tsc | 20.955560 / 20.954801 / 7.605462 | -0.0036% | 0.0114% | 255.9 / 256.3 / 235.3 | 1 / 1 / 1 |
| zod | 5.837398 / 5.837183 / 1.840575 | -0.0037% | 0.0051% | 32.4 / 33.0 / 82.6 | 0 / 0 / 0 |
| qs_parse | 4.925243 / 4.925239 / 1.553678 | -0.0001% | 0.0002% | 33.9 / 33.8 / 84.3 | 0 / 0 / 0 |
| qs_stringify | 10.709163 / 10.709207 / 1.957806 | +0.0004% | 0.0005% | 33.7 / 34.4 / 83.3 | 0 / 0 / 0 |
| commander | 6.763342 / 6.763344 / 1.458751 | +0.0000% | 0.0001% | 29.7 / 29.8 / 86.2 | 0 / 0 / 0 |
| fastify | 7.841249 / 7.841997 / 2.107183 | +0.0095% | 0.0224% | 75.2 / 79.3 / 135.8 | 0 / 0 / 2 |
| effect | 20.126961 / 20.126959 / 3.707851 | -0.0000% | 0.0074% | 135.8 / 138.5 / 260.8 | 1 / 1 / 2 |
| buffer_heavy | 9.998231 / 9.456513 / 9.103211 | -5.4181% | 0.0001% | 61.3 / 61.8 / 124.9 | 36 / 36 / 6 |
| worker_heavy | 1.797321 / 1.797580 / 2.809211 | +0.0144% | 0.6049% | 102.1 / 102.8 / 359.1 | 41 / 41 / 7 |

| Micro | Instructions G | Fix delta | Cycles G | Wall s | User s | RSS MiB | Full GC |
| --- | --- | --- | --- | --- | --- | --- | --- |
| worker-large | 1.389171 / 0.968134 / 2.123492 | -30.31% | 0.502 / 0.379 / 1.047 | 0.182 / 0.151 / 0.270 | 0.135 / 0.100 / 0.280 | 106.5 / 106.3 / 163.0 | 5 / 5 / 2 |
| worker-stream-large | 1.379918 / 0.958969 / 2.081116 | -30.51% | 0.482 / 0.363 / 0.981 | 0.163 / 0.130 / 0.226 | 0.129 / 0.094 / 0.266 | 75.6 / 75.5 / 153.9 | 4 / 4 / 1 |
| worker-all | 5.538953 / 4.563342 / 3.846040 | -17.61% | 1.932 / 1.661 / 1.839 | 0.692 / 0.622 / 0.501 | 0.521 / 0.457 / 0.505 | 178.6 / 178.5 / 233.7 | 9 / 9 / 3 |
| worker-stream-all | 5.581010 / 4.631579 / 3.791228 | -17.01% | 1.982 / 1.733 / 1.781 | 0.714 / 0.651 / 0.477 | 0.527 / 0.474 / 0.495 | 127.7 / 129.1 / 197.5 | 13 / 13 / 4 |
| hash-all | 2.558401 / 1.750610 / 2.285745 | -31.57% | 0.586 / 0.431 / 0.702 | 0.184 / 0.147 / 0.199 | 0.158 / 0.118 / 0.189 | 65.0 / 64.8 / 112.3 | 2 / 2 / 0 |
| inflate-large | 2.109403 / 1.215853 / 2.517154 | -42.36% | 1.076 / 0.733 / 1.386 | 0.404 / 0.337 / 0.371 | 0.293 / 0.203 / 0.368 | 124.5 / 141.4 / 159.7 | 3 / 3 / 5 |
| inflate-all | 4.764189 / 2.712599 / 4.830491 | -43.06% | 2.436 / 1.627 / 2.666 | 0.851 / 0.687 / 0.726 | 0.655 / 0.450 / 0.712 | 201.8 / 246.6 / 223.8 | 5 / 5 / 8 |
| parse-all | 2.841489 / 2.841413 / 1.364927 | -0.00% | 1.023 / 1.015 / 0.763 | 0.434 / 0.431 / 0.246 | 0.282 / 0.280 / 0.204 | 198.4 / 198.7 / 239.6 | 4 / 4 / 4 |

Ring 0.17.14 requires `(Avx, IntelCpu)` for x86 SHA-512, explicitly citing pre-Zen SHLD/SHRD costs. It has no SHA-512 AVX2 path to enable. This is a vendor gate, not missing feature initialization or target flags. The matched native probe (100 corpus passes, n=5 interleaved) gives:

| Provider | Instructions G | Locked cycles G |
| --- | --- | --- |
| ring portable | 49.974 | 10.756 |
| ring AVX with vendor gate removed | 41.533 | 16.622 |
| RustCrypto AVX2 | 61.299 | 10.788 |
| bundled OpenSSL | 33.827 | 7.540 |
| AWS-LC | 49.974 | 10.756 |

All digest bytes match. Production uses fixed `SHA512_CTX`, not EVP allocation/lookup, with bundled OpenSSL 3.6.3 through openssl-sys 0.9.117. Digest length is authoritative for SHA-384/512 and cloning; HMAC uses two of these contexts with RFC 2104 pads. Actual compiled Perry profiles select `sha512_block_data_order_avx2`. SHA-1/256 already use RustCrypto's SHA-NI routines in about 99% of their hash samples. KDFs, TLS and other algorithms are unchanged.

Actual one-shot inflate was flate2 1.1.10/miniz_oxide 0.9.1; streaming drove miniz directly. The native corpus probe (20 passes, n=5) gives miniz 20.166 G instructions / 9.308 G cycles, stock zlib 1.3.1 17.079 / 10.761, and zlib-ng 2.3.3 12.577 / 6.600. A zlib-rs 0.6.4 probe adds 5.8% instructions despite saving 19.7% cycles. zlib-ng wins both metrics. Only inflate changes; compression remains miniz. Node 24 uses zlib 1.3.1-470d3a2.

Sync/raw/gzip and streaming calls now use the same decoder and stable C context, owned through the existing BufferOwner allocator hook. Borrowed pointers are cleared after each step. The one-shot reader writes directly into its result Vec. Existing FFI input copying is unchanged. Streaming retains its bounded scratch and one JS buffer copy per output chunk. Allocation diagnostics show unchanged 1 MiB worker scratch: old native state requests 43,296 bytes; new state requests 42,112 plus a 104-byte stream. The unsafe zeroed miniz-state constructor is removed.

Perf cycle sampling uses retained symbols and joined stack/IP dumps. About 93–97% of worker sample periods have no usable call chain; their sampled functions are retained, but generic property/byte helpers cannot reliably be assigned to a caller. Batched large-worker user CPU is 118.9 / 90.3 / 134.8 ms per round. Weighting those totals by sampled periods estimates hash at 45.1 / 31.5 / 33.8 ms and inflate at 48.7 / 33.7 / 46.0 ms. These are approximate CPU attribution, not stage wall timers. The independent write trace on CPUs 56–63 under lock, three corpus passes, gives 27.74 / 27.42 / 25.71 ms aggregate syscall elapsed per pass. Perry performs 1,679 payload writes in both arms; Node adds one auxiliary byte across the batch. Elapsed includes kernel execution and waits, excludes metadata syscalls, and is diagnostic rather than an A/B speed claim. Original upm's within-part duplicate suppression accounts for 98,712 logical bytes not written per pass.

Pure tar parsing is unchanged. Its baseline full-stack samples assign 21.0% to cc's regex work, 6.1% to hwp's byte/views work and 2.3% to spreads/safePath. Direct tar/header/number processing is 17.1%, header strings 1.7%, property lookup 11.5%, GC/pacing 14.6%, allocation/copies 11.2% and promise/dispatch 11.3%. These costs were measured and left intact. Actual cold profiles also expose the metadata JSON scanner, property operations, GC and callbacks; their single-profile CPU totals vary and are not substituted for the locked timing medians.

`buffer_heavy` benefits from its existing gunzip workload; other policy program instruction deltas are within their measured floors except hello's tiny wrapped count. Hello's loaded code/data sections are identical, and a direct n=5 perf replay gives 111,946 instructions in both arms. The original count includes the timer process and variable serialization work. Lock mode’s +0.22% lies within its 0.284% repeated-baseline floor. Offinst’s initial +0.0185% just exceeds its 0.0166% floor, but an independent n=5 interleaved confirmation gives 1,552,441,157 / 1,552,534,691 / 1,926,913,033 instructions (+0.0060%), within that floor; no repeatable regression is claimed.

Per-process THP-off controls verify `THP_enabled=0` and `AnonHugePages=0`. Cold/lock RSS becomes 131.0 / 132.8 and 83.0 / 84.7 MiB, with full-GC medians 5/5 and 0/0. Additional n=5 cold/lock snapshots split the peak difference into about 0.86/1.33 MiB of resident file-backed pages and 0.66/0.55 MiB of anonymous allocator placement/transient work. Cold collection counts vary from 5–8, while lock remains at zero; the changed code placement and bounded working-set overlap explain these small peaks. Worker stress has variable collection overlap and a repeated-baseline RSS span of 98.4–104.0 MiB; its small default RSS delta lies inside that envelope. Fastify/Effect snapshots isolate about 4.0/2.7 MiB of extra resident **file-backed clean pages**, with anonymous deltas only +16/−8 KiB and unchanged full-GC counts. Source-stamped rebuilds change linked text placement; Effect's text size is unchanged. Other small program RSS shifts likewise mainly appear in file-backed pages.

Repeated one-shot inflate has a bounded memory tradeoff: +16.9 MiB large / +44.8 MiB corpus, persisting with THP off and identical full-GC counts. GDB/pagemap shows the same 75.156 MiB total reserved result capacity for 44.996 MiB output in both arms. Old short reads touch 46.531 MiB; bulk reads through the shared std Read adapter initialize all 75.156 MiB, making spare capacity resident and leaving more pages in the existing allocator. Worker scratch/state do not grow. A forward improvement is an initialized-output cursor in the shared decoder reader to avoid touching unused Vec capacity; it must retain one decoder mechanism. No issue was opened, as requested.

Tests: `cargo test --release -p perry-ext-zlib -- --test-threads=1` passes 32, with the existing 50,000-completion churn test ignored. Full stdlib passes 262 and fails exactly the same two thread-exit side-table tests as baseline (260 pass / 2 fail); skipping only those two passes 262. New crypto tests cover digest boundaries, clone at byte 113, chunks and HMAC key lengths around 128 against independent RustCrypto. Inflate tests cover moved contexts, raw/zlib boundaries, output guards, cleared pointers, exact release and refused allocations.

Node26 parity commands `./run_parity_tests.sh --filter ...`: `test_gap_crypto_wide` passes 2; `test_gap_crypto_byte_update_encoding`, `test_gap_crypto_hash_chain_11516`, `test_gap_zlib_cpu_inflate` and `test_gap_zlib_chunk_backpressure` each pass 1. `test_gap_native_payload_zlib_one_shots` fails only three compressed-byte CRC fields, an existing encoder gap: a separate main-flate2/miniz probe reproduces Perry's gzip/deflate/raw CRCs exactly. All decoded-data checks and remaining lines equal Node. No compression code changed. Node-version consistency, tracked file-size and whitespace checks pass.

Both delivered backend witnesses use the actual compiled Perry API and preserve Node parity:

| Witness | Old implementation median, n=5 | Combined median, n=5 | Limit | Outcome |
| --- | --- | --- | --- | --- |
| SHA-512, 16 corpus passes | 8,056,696,437 | 5,471,768,493 | 6.5 G | old 5 fail; fix 5 pass |
| Inflate, 5 corpus passes | 4,764,353,866 (SHA-only commit) | 2,712,760,433 | 4 G | old 5 fail; fix 5 pass |

Masking AVX2/BMI1/BMI2 with `OPENSSL_ia32cap=':~0x128'` preserves digests and fails the SHA budget at 8,056,744,621 instructions. The isolated SHA-only phase is preserved separately and is not mixed into these tables.

Open questions: the initialized-spare-capacity tradeoff above; broader CPU/platform coverage for the bundled providers; and unmodified entry/metadata/object/GC work, including cc/hwp's assigned items. Commits and compact raw evidence accompany `fix.bundle`; larger profiles, binaries and fixtures remain in `/root/lanes/perry-unpackcpu`. No push, PR or issue was made.
