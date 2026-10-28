Reduce string construction overhead by formatting numeric and inline-string concatenation operands on the stack. Reuse the converted bytes when allocation needs collection, with heap operands rooted and reloaded before copying. Preserve UTF-16 lengths, raw-byte provenance, WTF-8 surrogate repair and the established heap-string fast path. Avoid formatting numbers twice when their digits exceed inline-string capacity.

Repair stale unit fixtures for realm-image seeding, linked atom records and optional store auditing, and isolate the deserializer feature check from prior dynamic-code state.
