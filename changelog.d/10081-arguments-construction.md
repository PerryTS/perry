Reduce arguments-object construction overhead by building indexed fields in bulk,
sharing a bounded cache of immutable key layouts, and omitting redundant default
indexed-property descriptors. Batch the initial sloppy `length` and `callee`
descriptors into one shape transition. Arguments objects retain independent
identity, values, descriptors, and mapped parameter boxes. The key cache is traced
and rewritten by the arguments GC scanner, with regression tests for copy-on-write
keys and reachability through moving collections. Fixes #10063.
