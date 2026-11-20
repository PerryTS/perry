Compiled packages retain RegExp method behavior when they receive a regular
expression created by application code, including on macOS allocations below
2 TB. This covers schema-library paths such as zod regex and datetime checks
when compiling with the full prebuilt stdlib.
