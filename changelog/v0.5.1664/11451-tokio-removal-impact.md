**bench: measured what removing tokio changed** (`benchmarks/tokio-removal/`). Compares `d8f24f15ed`, just before the tokio-removal lanes, with `bd8936e98e`, after #11402. The arms are 732 commits apart, so the runtime rows cover the whole window.

- **Binary size:** I/O programs are 27–34% smaller on Linux and macOS (the `node:http` server drops from 18.1 to 12.3 MB).
- **Symbols:** tokio, hyper, reqwest, h2, tower and tokio-rustls counts are now 0 in every probe.
- **Archives:** `libperry_stdlib.a` −13.6%, `libperry_ext_http.a` −29.4%, `libperry_ext_net.a` −24.5%.
- **Dependencies:** `Cargo.lock` went from 1023 to 921 packages.
- **Build:** release archives take 7–11% less CPU to build.
- **Runtime:** http server peak RSS under load is 13–22% lower. Retired instructions per operation fell 17–23% for timers, net and async crypto.
- **Regressions:** http server throughput is −9% at c=256. AFTER also leaks one socket fd per in-process `http.get` round trip, so it hits `ECONNREFUSED` at the 1024-fd ulimit after about 1,015 sequential requests.
- **Threads:** unchanged at 1 in both arms.

No code, gate or version changes.
