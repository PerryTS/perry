**`perry-runtime` compiles again under `-D warnings` on linux-x86_64.** The crate
declared `pthread_getattr_np`, `pthread_attr_getstack` and `pthread_attr_destroy`
in three separate `extern "C"` blocks using two different spellings of the
`pthread_attr_t` buffer — `[u64; 8]` in `gc::roots::get_stack_bottom`, `*mut u8`
in `gc::roots::stack_maps::fp_chain` and in the Error-stack frame walk. Declaring
one symbol twice in a crate with different signatures is
`clashing_extern_declarations`, which `-D warnings` denies, and the crate stopped
building outright (#9486's frame walk supplied the third copy).

The `*mut u8` spelling was not itself new, but its only previous home was
`fp_chain`, which is gated to `target_arch = "aarch64"`; on an x86_64 host that
module is compiled out and the two spellings never met.

All three blocks now use `[u64; 8]`, the spelling the collector has always used.
That also clears the latent aarch64-linux collision between `fp_chain` and
`gc::roots`, and replaces a `[u8; 128]` buffer — 128 bytes but aligned to 1 —
with one that is both large enough for `pthread_attr_t` on every supported
glibc/musl target (56 bytes on both) and correctly aligned for it.
