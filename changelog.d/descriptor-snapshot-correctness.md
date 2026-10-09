Use one rooted, validated descriptor record for Object and Reflect definitions. Object.defineProperties snapshots all string and symbol keys, collects descriptors before applying, and preserves original proxy invariant facts when traps mutate their fresh descriptor copies.

Named array accessors retain rooted previous halves and rebound getters across closure allocation, and reload the receiver before installing descriptor metadata.
