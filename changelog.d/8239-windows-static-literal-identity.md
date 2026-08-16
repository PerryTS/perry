`windows-build` is green again, restoring the whole `test.yml` run that
`release-packages`' `await-tests` requires before it will cut a tag.

Two tests from #8177 failed on Windows and only there, asserting that a bound
method's closure captures the `'static` method-name literal by comparing
pointers. Both failing pairs differed by the same constant offset (0x161F90) —
two copies of the same read-only data, not a heap pointer. ELF
(`SHF_MERGE|SHF_STRINGS`) and Mach-O (`__TEXT,__cstring`) merge identical
read-only strings, so the copy the closure captures and the copy the lookup
returns land at one address; MSVC does not pool identical literals across
codegen units, so they stay distinct. Both are `'static`.

The address comparison is now gated on a linker that merges. The property the
test exists for — the captured name must not be the movable key string's
interior — is asserted unconditionally, as are the captured length and bytes, so
Windows keeps the coverage and loses only the proxy it cannot evaluate.
