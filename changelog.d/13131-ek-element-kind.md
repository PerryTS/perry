### Changed

- Arrays carry an element-kind word (the dense prefix: every slot below it is a plain data element). Reads inside the prefix, in compiled code and in the dynamic `obj[i]` inline cache, skip the bounds and hole tests; the runtime lowers the prefix at every descriptor install, and compiled stores raise it at the frontier. `ArrayHeader` grows from 16 to 24 bytes. The nursery ladder moves on its phase-mean influx so the larger header does not make it oscillate.
- `Array.prototype.fill` over an array with element descriptors runs the strict `Set` per index (accessor setters run, read-only elements throw).
