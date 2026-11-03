Fix quadratic non-ASCII `charCodeAt`, bracket indexing, `charAt`, and `at`
scans (#10055). A lazy sparse WTF-8 index and cursor avoid repeatedly decoding
the string prefix while preserving UTF-16 surrogate-half semantics and the
existing ASCII fast paths. The four-entry per-thread cache uses weak string
identities, follows GC relocation, and releases dead strings' indexing metadata
before their addresses can be reused.

Use the shared WTF-8 character builder for `at()` results as well, so indexing
an emoji returns its distinct surrogate halves instead of replacement characters.
