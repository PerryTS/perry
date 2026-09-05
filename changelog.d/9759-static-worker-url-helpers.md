### Fixes
- Resolve bounded module-local helper chains used as Worker entry URLs, including Bun embedded file URLs and `node:worker_threads` constructors. Substitute static string/URL arguments while rejecting effectful, mutable, recursive, opaque or over-budget helper expressions. Keep evaluating the original filename expression when constructing the Worker. Fixes #9744.
