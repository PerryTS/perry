# Pre-existing lazy stringify canonicalization discrepancies

Read-only correctness checks on the frozen main-eee, tape-depth R2 and R3
workers. Six inputs exceed the 1024-byte auto-tape threshold. Run each as
`worker FIXTURE roundtrip 1 0 verify`; Node 26.5.1 using worker.js is the oracle.
Auto and `PERRY_JSON_TAPE=0` run as separate processes, with other PERRY env
settings removed. No performance results are inferred from these local probes.

Whitespace, Unicode/slash escape spellings, duplicate object keys and reversed
integer-like object keys all produce incorrect auto-mode stringify output on
main and both candidates. All forced-direct comparisons pass, as does the
canonical auto-mode control. Thus these five failures predate the depth change;
they are not introduced by R2/R3. Raw output, inputs and binary provenance are
retained. Each of the 36 Perry comparisons has an explicit verdict.

The existing lazy shortcut normalizes number spellings but otherwise copies the
source range. Its comment requires canonical source form, but the examined
implementation does not establish that proof for these cases. A source-string
reuse optimization must first establish canonical-output equivalence or fall
back to ordinary materialization. No correction or new source-reuse behavior is
implemented in this diagnostic commit.

The standard stringify-only benchmark uses an object envelope to force eager
parsing. It does not exercise this lazy copy shortcut. These discrepancies
therefore do not explain the heterogeneous stringify CPU concern in that suite.
