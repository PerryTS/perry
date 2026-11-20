Long-running Fastify servers now recycle completed request-context handles.
The extension dropped one registry handle after every response, but only the
HTTP server pump drained the registry's one-tick quarantine. Fastify therefore
exhausted the 262,143-id common band under sustained traffic. The process event
pump now promotes retired FFI handles once at the beginning of an outer tick,
before any runtime, stdlib, or extension callback can retire another handle.
This covers every extension while preserving the same-tick ABA quarantine when
several extension pumps run sequentially.
