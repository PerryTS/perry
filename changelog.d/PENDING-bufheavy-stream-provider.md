No-auto builds take the stream dispatch features of the coherent
runtime/stdlib/wrapper rebuild, and the wrapper archives they link, from the
routed modules, the same selection automatic specialization uses. The rebuild
used to be an HTTP-only special case that also linked perry-ext-http for any
`node:http` import, even when PERRY_DISABLE_WELL_KNOWN=1 served it from the
bundled libraries.
