Prod-profile runtime archives (the ones releases ship) no longer prove 27 GC
call-effect rows weaker than workspace release archives do. Among them,
`js_eh_pad_enter` is a Leaf again and `js_inline_arena_slow_alloc` is
AllocOnly again: tsc compiled against prod archives drops 219 invokes and
1,237 safepoints. The runtime now reads those paths' thread-locals without
std's `LocalKey` accessor pointer (`perry_thread_local!`, a native
`#[thread_local]`, or the hot arena cache). UTF-8 validation calls the
AVX2/SSE4.2 validator directly instead of through simdutf8's dispatch pointer.
