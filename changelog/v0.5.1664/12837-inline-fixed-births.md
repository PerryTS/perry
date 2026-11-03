Inline nursery allocation for fresh fixed-size closures and compiler-known ordinary layout objects. Generated code reuses the existing cursor and writes the same headers, captures and hole slots as the runtime; a full nursery or active incremental birth color takes the original runtime allocator. Large cells and 32-bit targets retain runtime allocation. Closure identity and lexical captures are preserved.

Remove duplicate `requested_modules` fields from six codegen test initializers so allocation and rooting validation can compile.
