# R2 correctness and generated code

All 282 JSON unit tests pass at 0.5.1529. The matched compiler, runtime-static and
stdlib-static release build passes. Compiled output, retained lifetime, moving
collection and full-GC witnesses pass; see `gc-witness.json`. This development
host validation is not performance evidence.

Outlining the large allocation helper alone did not restore the shared function
size: `parse_string_value` still occupies 2,408 bytes with a 144-byte stack frame
(reference: 976 bytes / 96-byte frame). The normal counter is still inlined into
it. Any next revision must explicitly preserve that constructor boundary and be
rebuilt and measured independently.

This candidate is based on PR #10032 before the lazy-surrogate review fix. It
must not be presented as correctness acceptance for that separate issue.
