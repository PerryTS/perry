### TLS budget profiles now measure a live async workload (#8066)

The Darwin TLS gate's `asyncpipe` subject could finish during the fixed
one-second pre-attach delay. Its `_tlv_get_addr` share then came from an empty
or undersampled call graph: locally the unchanged checker rejected 521 roots,
while #8050 recorded the zero-root endpoint. The 5% budget was not the bug and
remains unchanged, as does the 2,000-root minimum.

Profile mode now repeats the same async/Map/Set/template-literal pipeline for
at least 12 seconds, checks every completed pipeline against the first, emits
the normal deterministic result, and reports how many pipelines completed.
The gate runs subjects in a clean environment, clears only their named output
artifacts before reuse, requires the process to be live and non-zombie before
attachment, rejects a nonzero sampler status, and checks the profiled stdout.
Compiler-free self-tests cover immediate exit, failed sampling, and an explicit
zero-root report. A local Darwin run completed two pipelines in 20 seconds and
collected 5,831 roots, with 17 `_tlv_get_addr` samples (0.3%), 152 claimed
generic slots, and `direct_tsd=1`; normal and profiled stdout had identical
SHA-256 `4059a6f4868d93a81f4064158c5a31bde6f89d1be2e683b01f2d34d45daef605`.
