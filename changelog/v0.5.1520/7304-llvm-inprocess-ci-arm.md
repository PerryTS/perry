CI arm for the in-process LLVM backend (#7301 follow-up, kill-policy): a
path-filtered macOS job builds the `llvm-inprocess` feature against a
loudly-pinned LLVM 22, runs the perry-codegen suite asserting the corpus
and RS4GC gates actually ran, and smokes `PERRY_LLVM_INPROCESS=native`
end-to-end — liveness line, behavior parity with the text arm, and
object-byte `=diff` verdicts for a single module and a forced 3-unit
split. Lands non-required; promote after first green.
