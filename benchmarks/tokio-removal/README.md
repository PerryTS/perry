# What removing tokio changed

This directory measures Perry before and after tokio was removed from the workspace
(#11402 and the tokio lanes before it). It holds the probe programs, the scripts that
built and measured them, the raw results, and a generated table
(`results/summary.md`, built from the raw files by `make_report.py`).

| arm | commit | what it is |
|---|---|---|
| **BEFORE** | `d8f24f15ed` (v0.5.1643, 2026-09-23) | turnloop was already the event loop (#10354), but tokio, hyper, reqwest, h2, tower, tokio-rustls and aws-lc were still linked in |
| **AFTER** | `bd8936e98e` (v0.5.1654, 2026-09-26) | `origin/main` after #11402. No tokio anywhere in `Cargo.lock` |

**Caveat: the two arms are 732 commits apart, and the tokio lanes are only some of
them.** Other changes in the window include the removal of three ext crates
(ioredis, mongodb, decimal), fetch and handle-registry fixes, and GC work. The
symbol census and the per-crate size attribution below show which bytes belong to
the tokio stack. The runtime numbers cover the whole window and should not be read
as the effect of tokio removal alone.

For the event loop itself (turnloop vs tokio's wait driver, measured at one commit),
see the earlier A/B at `650ea6d661`. It found +18.9% throughput at c=1 and −57.3%
memory per idle keep-alive connection. That comparison could not change binary
size, because both of its arms still linked tokio. Removing tokio is what this
measurement adds.

## Headline

Sizes are Linux x86_64 release builds with auto-optimize ON (the default user path
from a source checkout), stripped. Runtime numbers come from the quiet M1 bench mini
(macOS): 7 interleaved A/B rounds, 10 s per load cell, median values.

| metric | BEFORE | AFTER | Δ |
|---|---:|---:|---:|
| `node:http` server binary (Linux) | 18.14 MB | 12.28 MB | **−32.3%** |
| `node:http` server binary (macOS arm64) | 14.93 MB | 9.73 MB | **−34.8%** |
| mixed backend binary (http + fetch + crypto + zlib + timers, Linux) | 24.09 MB | 17.26 MB | **−28.3%** |
| fetch client + local server binary (Linux) | 20.40 MB | 13.55 MB | **−33.6%** |
| `node:net` echo binary (Linux) | 13.71 MB | 9.96 MB | −27.3% |
| timers-only binary (Linux) | 8.62 MB | 7.28 MB | −15.5% |
| any program linking the prebuilt full stdlib (`PERRY_NO_AUTO_OPTIMIZE=1`) | 30.78 MB | 25.54 MB | −17.0% |
| tokio / hyper / reqwest / h2 symbols in the http server | 1362 / 1012 / 380 / 956 | **0 / 0 / 0 / 0** | gone |
| `libperry_stdlib.a` (prebuilt, full) | 99.97 MiB | 86.41 MiB | −13.6% |
| `libperry_ext_http.a` / `_net.a` / `_ws.a` | 95.5 / 52.6 / 58.8 MiB | 67.5 / 39.7 / 50.3 MiB | −29.4% / −24.5% / −14.4% |
| `libperry_runtime.a` | 48.42 MiB | 48.33 MiB | −0.2% |
| `Cargo.lock` packages | 1023 | 921 | −10.0% |
| deps reachable from perry-ext-http / perry-ext-net / perry-stdlib | 177 / 48 / 404 | 134 / 33 / 365 | −24% / −31% / −10% |
| cargo build of perry + runtime + stdlib archives, CPU time | 1,856 s | 1,718 s | −7.4% |
| same, plus every ext crate, CPU time | 2,084 s | 1,849 s | −11.2% |
| cold `perry compile` of the http server, CPU (auto-opt rebuild) | 649 s | 552 s | −15.1% |
| http server req/s, c=1 / 64 / 256 | 20,579 / 99,507 / 107,081 | 20,404 / 100,507 / 97,422 | −0.9% / +1.0% / **−9.0%** |
| http server p50 / p99 latency, c=64 (ms) | 0.631 / 0.872 | 0.629 / 0.867 | −0.2% / −0.7% |
| http server CPU per request, c=1 / c=256 (µs) | 26.7 / 9.2 | 27.8 / 10.1 | +4.0% / +9.8% |
| http server peak RSS under load, c=1 / 64 / 256 | 32.0 / 32.9 / 35.4 MiB | 25.0 / 28.1 / 30.7 MiB | **−22% / −15% / −13%** |
| http server idle RSS | 14.4 MiB | 14.1 MiB | −2.3% |
| mixed backend req/s, c=1 | 12,731 | 12,389 | −2.7% |
| fetch client req/s, c=1 / c=16 | 17,447 / 2,685 | 19,049 / 64,779 | +9% / **24×** (see notes) |
| fetch client peak RSS, c=1 | 25.4 MiB | 22.4 MiB | −11.9% |
| threads: idle, under load, and in every probe | 1 | 1 | unchanged |
| startup, hello (warm) | 1.56 ms | 1.56 ms | 0% |
| startup, backend to listening (warm) | 7.48 ms | 7.13 ms | −4.7% |
| retired instructions per op: timers / net round trip / async crypto / async zlib | 33.3k / 76.6k / 94.6k / 224.5k | 25.7k / 59.6k / 78.1k / 203.3k | −23% / −22% / −17% / −9.5% |
| retired instructions per op: fetch round trip | 426k | 420k | −1.5% |
| bare-loop control (pure JS) | 38,000 | 38,001 | 0.0% (the control holds) |

## Biggest wins

- **Binary size of I/O programs: −27% to −34% on Linux and macOS.** The attribution
  (`results/summary.md`) shows where the bytes came from in the http server:
  aws-lc (−1.46 MB of symbols plus about 0.8 MB of unmangled C), h2 (−384 KB), hyper
  and hyper-util (−387 KB), tokio (−214 KB), and reqwest (−102 KB). Programs that use
  `fetch` also lose 828 KB of brotli encoder. No crate replaced any of this:
  turnloop's share is flat at about 160 KB in both arms, because turnloop was already
  linked at BEFORE. In `node:net` programs, `ring` replaces aws-lc as the rustls crypto
  provider (+211 KB of C against −1.46 MB).
- **The tokio stack is fully gone.** Every probe has 0 tokio, hyper, reqwest, h2, tower
  and tokio-rustls symbols, and 0 `tokio-1.x` panic-location strings. At BEFORE the
  counts were 186 to 1434 tokio symbols wherever any I/O module was linked. The six
  `mio` symbols left at AFTER are mio 0.8 from `notify` (the file watcher), not an
  async runtime.
- **Memory under load: −13% to −22% peak RSS** for the http server, and −12% for the
  fetch client. Idle RSS moves only about 2%.
- **Event-loop work per operation fell** by 17–23% for timers, net and async crypto, and
  by 9.5% for zlib, while the pure-JS control did not move.
- **Concurrent fetch:** 16 concurrent `fetch()` calls against a local Node server went
  from 2,685 to 64,779 req/s. The BEFORE number held at about 2.7k in all 7 rounds, so
  it is not noise. BEFORE's binary was the prebuilt-fallback link (see notes). The
  cause of the BEFORE slowdown was not investigated.
- **Build:** 102 fewer lockfile packages, and 7–11% less CPU for the release archive
  build. Cold `perry compile` of I/O programs uses 15–18% less CPU, because the
  per-program runtime and stdlib rebuild no longer compiles the tokio/hyper/aws-lc
  graph.

## Regressions and anomalies (first-pass reasons)

1. **AFTER leaks one socket fd for every completed in-process `http.get` round trip.**
   `ops_http` runs sequential `http.get({agent:false})` requests against a server in
   the same process. After 300 requests it holds 301 socket fds at AFTER, against 6 at
   BEFORE. The fds are not in any TCP state (`ss` lists only the listener), so the
   socket was never `close()`d. At the default `ulimit -n 1024`, AFTER dies with
   `connect ECONNREFUSED` after about 1,015 requests (fails 3/3 at N = 2,000 and at every N tried from
   1,020 to 1,700; passes at N = 1,010, and at N = 2,000 with `ulimit -n 8192`). BEFORE and Node complete N = 2,000.
   Instruction counts for this probe are superlinear in N in *both* arms (see the ‡
   note in `summary.md`), so its per-op number is not reported.
   A related effect exists in both arms: a perry `node:http` server driven by `curl`
   (a new connection per request) still holds one fd per accepted connection after the
   client has gone (206 fds after 200 connections). The oha load cells use keep-alive,
   so they do not hit this.
2. **http server at c=256 is −9% req/s, and CPU per request is up +4% at c=1 and +10%
   at c=256.** AFTER sits at about 97.4k req/s in 6 of 7 rounds. BEFORE reaches about
   107k in 4 of 7 rounds and dips to 50–92k in the others. At c=64 the arms are equal.
   The window contains non-tokio server-path changes as well (handle-registry and
   fetch fixes), and this run does not separate them.
3. **The mixed backend is −2.7% req/s at c=1 (+4.3% CPU per request).** At c=8–64 the
   arms are within −0.1% to −3.1%.
4. **Pre-existing in both arms, and not tokio-related:**
   - The backend server panics with `common native handle registration exhausted:
     IdExhausted` (at `crates/perry-stdlib/src/common/handle.rs`) partway through the
     c=64 cell, after about 200k requests in one process (c=1 for 10 s, then c=64). It
     happens in all 7 rounds, in both arms. A fresh process per cell survives 5 s at
     c=64 (about 175k requests). The c=64 cells from the main run are therefore excluded, and fresh-process
     cells replace them.
   - `ws` (client + server echo) hangs in both arms (#11309).
   - `worker` never delivers `postMessage` to the parent (it prints `worker exit 0`
     without `main got 42`).
   - `perry/container` (with a stub `docker` on `PATH`): at AFTER `getBackend()`
     returns a string and `list()` resolves. At BEFORE `getBackend()` was not a string.
     This is an improvement.
   - BEFORE's auto-optimize cargo build fails for the `async-runtime,web-fetch` feature
     set (`unresolved import base64` in perry-stdlib) and falls back to the prebuilt
     full stdlib. The standalone fetch-client probe's BEFORE auto-optimize size is
     therefore its prebuilt size (marked † in `summary.md`). `fetch_local` is the
     like-for-like fetch row.

## What was measured, and how

| what | where | how |
|---|---|---|
| sizes, symbol census, attribution, archives | perrymaster (Linux x86_64) | `--release` (thin LTO, codegen-units=1; this is also the profile auto-optimize rebuilds with — `dist` differs only by `strip=true` on non-runtime crates), clean clone of each commit, one target dir per arm, `-p perry -p perry-runtime-static -p perry-stdlib-static`, then the same plus every governed `perry-ext-*` in one invocation (#7358) |
| cargo build time | perrymaster | both arms concurrently at `-j4 nice 10`, load 35–75. Wall time is indicative only; CPU time (user+sys) is the comparable number |
| `perry compile` time | perrymaster (+ macOS dev MacBook) | cold = empty object cache and a feature set not yet built. Auto-opt dirs are kept only between siblings with the same feature set, so a `rebuilt=0` row is a cache hit and says so |
| runtime | bench mini (M1, macOS 26.5.1, oha 1.16.0, hyperfine 1.20.0) | binaries compiled on the MacBook with auto-optimize ON and shipped to the mini. The shared mini bench mutex `/private/tmp/perry-bench-lock.d` was taken for every run |
| instructions | perrymaster | `perf stat -e instructions:u`, 7 reps, arms interleaved, N = 100 and 500, per-op = ΔI/ΔN, `ops_loop` as the pure-JS control |
| correctness | both hosts | each probe's output compared with Node 26.5.1 (`/opt/node-v26.5.1-linux-x64`, `~/nodebin/node`). All probes match in both arms except the pre-existing `ws`, `worker` and `container` cases above. `timers` matches on both hosts |

Thread counts use `ps -M` sampled every 100 ms during load (verified against `node`,
which shows 7 threads). **Both arms run every probe on one thread.** At BEFORE,
turnloop already carried all I/O and the tokio runtime was never started on these
paths, so removing tokio cannot lower the thread count further.

## Not measured

- Windows, and Linux runtime (wall-clock) numbers: perrymaster is shared, so Linux
  results are sizes and retired instructions only.
- The optional pre-turnloop reference (the parent of `b77aba6343`). The event-loop
  effect is cited from the `650ea6d661` A/B instead.
- The tls client under load, https under load, WebSocket throughput (the probe hangs in
  both arms), and HTTP/2.
- macOS size attribution and archive sizes. macOS program sizes are in the
  compile-time table.
- BEFORE macOS `fetch_local`: its compile was terminated (rc 143) and was not re-run.
  It was not one of the mini's runtime probes.
- `cargo bloat`: attribution uses `nm -C --size-sort -S` symbol sizes, which leave out
  padding and unwind tables. Use it for shares, not absolute totals.

## Files

- `probes/`: every program, plus a stub `docker` and the `ws` fixture (`npm install`
  pins `ws@8.21.0`).
- `scripts/build_arm.sh`: builds one arm on Linux.
- `scripts/compile_probes.sh`: compile census for one arm (sizes, symbols, archives,
  compile time, output vs Node).
- `scripts/instr.sh`: instruction-count A/B.
- `scripts/attribute.py`: per-crate size attribution from `nm` output.
- `scripts/deps.py`: lockfile and dependency footprint.
- `scripts/build_mac.sh` and `scripts/compile_mac.sh`: macOS arms.
- `scripts/bench_mini.py`: the runtime A/B.
- `scripts/diag_mini.py`: fresh-process backend cells, fetch reliability, and peak
  thread counts.
- `scripts/make_report.py`: renders `results/summary.md` and `results/summary.json`
  from the raw files.
- `results/`: raw data (`linux-*.json[l]`, `mac-compile.jsonl`, `mini.json`,
  `mini-diag.json`, the bench logs) and the generated summary.

To regenerate: `python3 scripts/make_report.py results`.
