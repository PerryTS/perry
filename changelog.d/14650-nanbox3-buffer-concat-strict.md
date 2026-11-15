`Buffer.concat(list)` decodes each list element by its tag alone. An element
without a NaN-box tag was kept as a buffer address and a header was read there,
so an address-shaped Number in the list (`Buffer.concat([e])` for
`e = Number.MIN_VALUE * 2 ** 28`, also its negative) crashed with SIGSEGV. Such
an element now throws ERR_INVALID_ARG_TYPE like every other Number, as in Node.
