**Lint: convert #8299's new bare handle reads in `webassembly.rs`.**

#8299's `WebAssembly.instantiate` result assembly reads its rooted handles through
bare `get_raw_mut_ptr`, taking `webassembly.rs` to 25 against its per-module
ceiling of 23 and leaving `main` red on the raw-handle debt ratchet — which is
the required `lint` context. Two argument-position reads are converted to the
#7341 blessed `with_mut_ptr` form, which never binds the pre-call address.
Total returns to 983 against a baseline of 983.
