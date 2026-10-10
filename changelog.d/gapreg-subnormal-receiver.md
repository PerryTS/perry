A dynamic `Number.prototype.toString` call on a positive subnormal number (for
example `x["toString"]()` with `x = 1e-310`) no longer crashes. The method asked
whether its receiver was a `new Number(...)` wrapper by reading a header at the
address the number's bits spell; it now accepts a bare word as a wrapper only
when the allocator owns it.
