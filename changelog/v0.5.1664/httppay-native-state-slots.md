Move standalone HTTP ServerResponse JS metadata into fixed, traced slots reached through the object's native-state record. Socket references, pending write callbacks and terminal metadata no longer traverse the hidden native-payload property or perform internal by-name reads and stores. User-selected event names continue to use the ordinary event dictionary, reached through a fixed slot.

Stream mode getters now read record slots. High-water-mark initialization reuses the resolved object mode instead of reading constructor options twice. Remaining literal keys use the existing canonical-key atom machinery, removing the stream-specific SipHash key cache, its scanner and its registration latch.

Heap receivers are rejected by the HTTP server dispatch probe from their value encoding before the registry is consulted. Actual servers still use numeric FFI ids and retain their existing registry validation; converting their constructors and lifecycle to object brands remains separate work.

Architecture witnesses include test-only sabotages that restore named state access, fresh key allocation and duplicate mode reads and server registry probing. No version fields change.
