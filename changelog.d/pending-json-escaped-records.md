Use the canonical JSON escape decoder for sparse tape reads and iterative
materialization. Preserve lone UTF-16 surrogate values and their string metadata
across isolated, ascending, descending and forced-materialization reads. Retain
the batch-specific copied-minor witness landed in #10033.

Quote lone-surrogate property names through the existing byte escaper instead of
emitting a synthetic field name when an object contains nested values. Reject
malformed internal key bytes before mutating the output buffer.
