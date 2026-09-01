**fix(release): exclude the musl targets from the release matrix (tracked by #9382)**

Both musl legs cannot build. `rusqlite`'s `session` feature pulls
`libsqlite3-sys/buildtime_bindgen`, so bindgen runs in a build script and
`dlopen`s libclang — but cargo builds build scripts for the **host**, and inside
the Alpine container the host *is* musl, whose `crt-static` default produces a
binary that cannot `dlopen`.

Two earlier layers were genuinely fixed on the way here (#9353 static system
libs, #9357 libclang), and a `[host] rustflags = -crt-static` split does get the
build past bindgen — but host and target are the same triple, so the relaxation
reaches the target and ships a dynamically linked "static" binary. The
staticness assertion added in #9357 caught that both times.

The real fix is cross-compiling musl from a glibc host, which needs musl-target
LLVM 22 in a glibc image. That is #9382, not a release-day change.

Excluding musl keeps the other six platforms shippable. The two musl entries are
also removed from the wrapper's `optionalDependencies`, `stage-npm.sh`'s
`PLATFORMS`, `PLATFORM_PACKAGES` and the npm freshness manifest — so an Alpine
user resolving the new version gets an explicit unsupported-platform error
rather than a silent missing binary, and can pin `0.5.1220`.
