**Receiver proof state now has one lifecycle owner** (#9254 phase 4). Codegen's
cached array lengths, bounded indices, packed-f64 loop facts, masked-window
facts, and native buffer views now live as typed payloads in the shared receiver
descriptor table. Together with the poll-refreshed receiver addresses migrated
in phases 2 and 3, this retires all six independent mechanisms named by the
receiver-region proposal.

The table now owns nested dynamic extents, lexical-scope teardown, reassignment
invalidation, and non-moving buffer-view address contracts. Existing fast-path
producers and consumers keep their established fallback behavior, while tests
pin nested-loop restoration, shared scope cleanup, temporary view replacement,
and moving-GC refresh behavior.
