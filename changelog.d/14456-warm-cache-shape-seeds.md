A second compile of a program with a warm object cache failed to link
(`corrupt static shape seed sidecar`, then `_main` undefined) whenever one of
its object literals stored an integer in an I32 lane, for example
`rows.push({ j, text })` with a loop counter `j`: the seed sidecar reader took
the I32 lane for a ConstFn lane without its body. The reader now reads I32
lanes, a cache entry whose sidecar does not decode is a cache miss that
recompiles the module and rewrites the entry, a seed the sidecar cannot carry
exactly is never cached, and a module that fails for any reason fails the
build instead of linking without its object.
