### Reduce compiler memory retained through code generation

Release batch SWC parse-cache entries after the final metadata-lowering pass while preserving dev/watch reuse. Normalize namespace export edges through borrowed declaration views instead of cloning complete HIR modules. Retain function, class and method source as immutable shared UTF-8 ranges, and borrow unchanged text during code generation, preserving exact source rendering and content-based cache hashes.
