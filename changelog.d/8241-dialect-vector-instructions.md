Fixed the in-process LLVM backend failing every module large enough to split
across native codegen units (#8228). Perry's dialect reader — the hand-written
parser the native path round-trips its own textual IR through — had no
`insertelement` case, so the `<2 x i64>` object-header image that #8204 began
composing at module init fell through to the binary-op arm and reported
``bad binary op `insertelement` operands``. The native path is the default for
split modules only, so this surfaced on exactly the biggest modules of a large
app: the pinned production Next.js App Route fixture could not build its app
dylib at all, five modules deep including the route runtime itself.

The reader now accepts the closed set of vector forms perry-codegen actually
emits: `insertelement`, `extractelement`, `shufflevector`, vector-typed `add`
and `mul`, constant vector literals, and `poison`/`undef`/`zeroinitializer` of
vector type. That also covers `expr/channel.rs`'s `<4 x i32>` SIMD byte-channel
reduction, which was the same latent bug and had simply never appeared in a
module big enough to split. `insertvalue`/`extractvalue` are deliberately still
rejected — codegen emits neither, and an unreachable arm is untested code.

The reason this could ship green is that the reader's only gate was a set of
frozen `.ll` snapshots under `experiments/`, which by construction cannot see a
form the emitters started producing later; and no per-PR job compiles anything
large enough to take the native path. Replaced with a live emit → re-parse test
that compiles a real module through the real emitters and feeds every function
it produced back through the reader, so the next new emission form fails in
`cargo-test` instead of in a user build.

Interim workaround for anyone on a released build: `PERRY_LLVM_INPROCESS=off`
selects the text transport, which is unaffected.
