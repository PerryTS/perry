Masked `string[]` length-accumulation loops now validate their receiver and
complete index window once, then load boxed string slots directly and read SSO
or heap-string lengths inline. The generic loop remains as the fallback for
erased annotation lies, holes, descriptors, prototype pollution, and
non-number accumulators.

On the issue-shaped `strings[i & 3].length` benchmark this lowers Perry from
5.58 to 1.14 ns/access on the same host (4.9x faster), reducing the gap to
Node from about 8.9x to 1.8x.
