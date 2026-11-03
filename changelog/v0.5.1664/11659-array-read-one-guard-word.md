Array element reads (`xs[i]` on an array receiver) now check one GC header
word for the whole structural guard (an ordinary array, not a growth stub, no
element descriptors), bound the index by the backing store's capacity, and
consult the prototype chain only when the slot is a hole or out of bounds. The
guard no longer carries the 16M-element plausibility compares, and a stale
alias of a grown array is followed off the hot path. On `xs[k & 7]` loops the
read drops from 64 to about 22 instructions.
