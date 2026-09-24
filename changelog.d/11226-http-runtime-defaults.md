Preserve the runtime's default engines when the no-auto HTTP client build rebuilds the stdlib and HTTP extension. Select `perry-runtime` in the same Cargo invocation so the runtime bundled into the stdlib archive retains regex, URL, Temporal, and the other default features. Add a Cargo feature-unification regression and an HTTP-importing native fixture covering RegExp methods and default runtime APIs.

Run the HTTP default-engine fixture explicitly in no-auto mode, retaining the Node comparison and allowing the compiler to build matching HTTP pump archives within the toolchain timeout.
