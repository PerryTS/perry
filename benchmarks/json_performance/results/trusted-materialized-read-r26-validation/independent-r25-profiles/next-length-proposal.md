# Next large-token length proposal — not applied

The R25 rotating profile attributes 373/758 Unicode-loop self samples to UTF-16 counting, 163 to string termination scanning and 116 to nesting preflight. The fixtures are small objects with an id and one large text field, not top-level scalar strings. A root-scalar nesting bypass already exists and does not solve this object case.

For a large unescaped token occupying nearly all of an input, a bounded ASCII check on the bytes before and after it can allow `source.utf16_len - outside_byte_count`. The production helper must prove that the parser input is the exact current source-string payload (same data pointer and length); no unchecked source identity assumption. A conservative final-three-byte lead-width guard must reject any WTF-8 lead that could cross the token boundary. The ordinary scanner still validates token terminators and raw controls. Escaped tokens, non-ASCII outside bytes, wide prefix/suffix, mismatched source payload or ambiguous final sequence use existing counting.

A standalone model reuses the exact runtime scalar WTF-8 counting function. It checked 3,065,796 arbitrary-byte/Unicode cases; 1,473,719 admitted hints equal the actual token count and 1,592,077 safely fall back. This is model evidence only: source-header correctness, borrowed vs owned token integration, source slicing, GC custody and generated code remain to be tested. Do not assume UTF-8 validity and do not replace the general WTF-8 counter. No production GC/core changes proposed.

A separate opportunity is to reduce repeated scans in nesting and string-token validation, but the object-root nesting proof cannot be skipped merely because a large leaf looks like text. Preserve malformed-input and recursive-stack protection tests.
