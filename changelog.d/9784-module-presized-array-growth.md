Fix out-of-bounds numeric array accesses when a static `new Array(n)` length exceeds the runtime’s dense allocation limit. Large module-level fills now use the guarded storage-growth path.
