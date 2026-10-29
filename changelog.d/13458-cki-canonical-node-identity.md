Canonical key lists are identified by their trie node, carried by every
holder (the canonical handle, the receiver's keys view and the shape record,
where it is packed beside the element kind in the existing 128-byte record)
instead of being looked up in an address-keyed map. The `(address, count)`
index of the canonical trie is removed; a carried node is validated by one
read, so a pruned or reused node can never answer for another list. The
prefix of a carried list is reached through parent links with no hashing.
Symbol keys hash their symbol serial rather than their address, so the edge
table no longer depends on link layout and a moved symbol still reaches its
edge. Worker shape seeds re-derive each list in the worker's own trie.
Startup Ir drops on every startup witness, and the canonical code's Ir no
longer moves when the image layout is padded.
