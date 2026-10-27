Qualify Perry-authored WASI 0.2.12 HTTP components on Linux with incoming plain HTTP/TLS, outgoing binary echoes, capability denials, cancellation, retirement and independent host/guest moving GC. The acceptance harness checks actual movement and evacuation, plus native from-space protection.

Copy canonical byte lists into a freshly rooted Uint8Array in bulk. This avoids exhausting a one-billion-instruction budget while lifting a 3 MiB argument through per-element setters. Unit coverage verifies independent storage and survival through moving collection.

The authored guest matrix passes all 12 phases with matching native artifacts and a byte-identical fresh guest compilation. Production runtime archives, other platforms, native WASI 0.3, released Turnloop dependency adoption and final packages remain pending; see `docs/validation/wasi-http-guest-20261023.json`.
