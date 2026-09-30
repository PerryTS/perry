### Performance

- Loop regions can use shape-proven F64 field reads in Number arithmetic, including values carried through local variables. Learned regions refuse receivers whose requested field is not an identity F64 lane and use the generic path for them.
