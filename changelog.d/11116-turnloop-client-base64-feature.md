Fix `perry-stdlib` failing to compile for any feature set that enables `turnloop-http-client` without `crypto`, which silently put every fetch program onto the full prebuilt runtime.

`turnloop_client::decode_basic` (proxy `Basic` credentials) uses the `base64` crate, but the `turnloop-http-client` feature never enabled `dep:base64` — only `crypto` did. The function is not `cfg`-gated, so it compiles whenever `turnloop-http-client` is on. Full builds enable `crypto` and compiled fine, which is why nothing caught it; the break landed with #10354.

The consequence was larger than a compile error. Auto-optimize builds a feature-stripped runtime per program, and a program using `fetch` resolves to `async-runtime,web-fetch`. That set failed to compile, and auto-optimize's recovery is to print a warning, fall back to the prebuilt full-feature archive, and exit 0 — so the program still built and ran, just linked against everything. Measured on main (macOS arm64, auto-optimize path, bytes from the linker map): a baseline program is 7.43 MB. The same program plus `fetch` came out at **23.63 MB** before this fix, carrying code it has no use for — sqlite, the SWC parser, Temporal, ICU. With the fix it gets its own stripped runtime and is **11.96 MB**, with sqlite, SWC and Temporal at zero. That is about 11.7 MB, roughly half, off every program that uses `fetch`.

Note that the fallback did print a warning, but the build succeeded and the binary ran correctly, so nothing downstream treated it as a failure. The measurement that exposed this only did so because it asserted the stripped runtime was actually built, rather than trusting the exit code.

The fix adds `dep:base64` to `turnloop-http-client`. It belongs there rather than on `web-fetch` because the use site compiles under `turnloop-http-client` alone, which the module documents as a supported combination.

Verified by reproducing the failure on main (`error[E0432]: unresolved import base64`) for `--features async-runtime,web-fetch`, then confirming it compiles with the fix. `Cargo.lock` is unchanged, since `base64` was already resolved.

Not addressed here: `--features turnloop-http-client` on its own still fails, with a different error (`cannot find async_bridge in common`). No real configuration reaches it — only `web-fetch` enables `turnloop-http-client`, and `web-fetch` already includes `async-runtime` — and resolving it by adding `async-runtime` would pull tokio into the turnloop client path.
