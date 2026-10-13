Use the runtime's existing composite-word hasher for canonical-key trie
edges and backing/count identities. This removes repeated SipHash byte
processing of internal numeric identities while preserving both tuple
fields and the trie's exact slot-content validation. No additional cache,
table, or generated per-site code is introduced.

On qb6, the descriptor-append micro drops from 95,668 to 90,473 instructions
per iteration (5.4%), with Node at about 17,900. The ordinary assignment
negative control stays flat because its existing transitions bypass these
maps. The fix-only fastify run saves approximately 16,500 instructions per
inject after startup subtraction. Its emitted text grows by 64 bytes.
