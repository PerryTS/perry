### Make Set lookup indexes lazy and stable across GC moves

Sets with at most eight live elements use the elements buffer directly for every value type. Larger Sets reach an owned index through `SetHeader.meta.native_state`, removing the per-Set `SET_INDEX` hash-table entry. Compact 8-byte buckets store a hash and element position; equality reads the authoritative elements buffer. Growth reuses cached hashes, and clear or shrinking to eight elements releases the index.

Movable pointer keys receive lazily assigned, thread-local weak identity tokens shared across indexed Sets. The registered metadata scanner rekeys these tokens without rooting their owners, and the dead-owner sweep prunes them. String hashes are content-based. Neither key relocation nor Set relocation rebuilds the index or rehashes strings. The existing allocation registry retains responsibility for freeing element buffers and the optional index; index bytes participate in external-allocation accounting.

Coverage includes mixed-value threshold transitions, forced hash collisions, ordered mutation churn, actual object/string/array evacuation without rehashing, repeated key relocation, weak-key death, and index-owner reclamation.

Validation: the full runtime unit suite passed (4,434 passed, 4 ignored), along with formatting, file-size, address-classification, GC-root-holder, Node-version consistency, and locked workspace metadata checks.
