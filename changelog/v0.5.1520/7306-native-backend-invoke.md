### In-process LLVM backend: construct `invoke`/`landingpad`/personality (#7302 follow-up)

The exception-lowering migration (#7302/#7305) made every `try`-containing
function — and every `async` function, via the rejection boundary — carry a
`personality` clause and `invoke`/`landingpad` instructions. The in-process
LLVM reader (`dialect.rs`, #7301) could not construct those forms, so #7305
routed such modules to the textual path with a module-level bail. That bail
turned the `native-backend` job red: its subject (`spike.ts`) is async, so
native construction never activated and the job's own liveness assertion —
`grep "in-process LLVM backend active"` — correctly failed.

Fixed properly rather than by narrowing the gate:

- `parse_header` lifts the `personality ptr @NAME` clause out of the define's
  attribute list and `begin` applies it via `set_personality_function`
  (previously the attribute loop hit `personality` and bailed as an unknown
  attribute).
- New `invoke` construction (value and void forms), sharing the callsite-typed,
  call-through-pointer semantics of the existing `call` path and adding the
  normal/unwind edges.
- New `landingpad` construction for the one shape Perry emits — a catch-all
  `{ ptr, i32 } catch ptr null` whose result is unused (the thrown value comes
  from the runtime's rooted TLS slot). Any other shape bails loudly.
- The module-level bail and its `has_eh_personality` helper are deleted; the
  per-function textual path in `native_emit` now feeds the reader rather than
  falling back to clang.

Verified locally against LLVM 22.1.4: liveness assertion restored, `spike.ts`
and the 3-unit `batch.ts` arms emit **byte-identical objects** on both paths,
and a full try/catch/finally program compiles through native construction with
output identical to the textual path.

Gate hardening (so this class of regression is caught by the gate that owns
it, not by a downstream merge):

- `dialect::tests::corpus_exception_handling` round-trips a tracked
  try/catch/finally corpus (`eh_text.ll`) through native construction and the
  LLVM verifier, and asserts the corpus still *contains* invoke edges, a
  landing pad, and the personality clause — a corpus that lost its EH forms
  would otherwise keep passing while testing nothing.
- The `native-backend` workflow gains an EH arm: liveness + behavior parity +
  object-byte diff verdict on a try/catch program, alongside the async spike.
