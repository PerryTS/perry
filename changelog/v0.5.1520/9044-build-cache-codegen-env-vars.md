`PERRY_BOX_CAPTURE_ENTRY_CELLS` and `PERRY_GUARDED_PREINLINE_MAX_IR_BYTES` are
now build-cache inputs.

Both landed without build-cache registration, so
`codegen_env_vars_are_build_cache_inputs` failed on `main` — and because it is a
`perry` bin-crate unit test, the whole `perry` test binary failed to compile,
turning every open PR's cargo-test job red.

Both are inputs rather than exclusions because both change emitted code: the
capture-cell knob changes every closure body that qualifies for entry-resolved
box cells, and the preinline ceiling changes which functions inline, so a run
with a raised ceiling must not be served objects a default run produced. That
is #6394's rule — a codegen env var keys the cache or carries a written reason
it cannot affect output.
