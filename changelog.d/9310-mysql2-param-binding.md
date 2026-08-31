Fix silent data loss in the bundled `mysql2` prepared-statement bridge. Perry
stores strings of at most five bytes in its inline SSO representation, but the
wrapper tested only for heap strings; every short string therefore fell into a
catch-all branch that deliberately emitted SQL NULL. A parameter list made of
short values and nulls arrived at MySQL as all nulls, while longer strings and
primitive numbers took their intended branches.

The bridge now accepts both Perry string representations without allocating in
the runtime heap and copies every parameter into an owned Rust value before
scheduling database work. Strings, integer and floating-point numbers,
booleans, null, Date, Buffer, and Uint8Array bind with their real values. An
undefined or unsupported parameter rejects the operation with an Error instead
of being rewritten to SQL NULL. Integer results are decoded with their actual
MySQL width and signedness, so a bound boolean no longer comes back as null.
Async `createConnection` and `getConnection` results preserve Perry's registry-
handle tag rather than returning an unusable ordinary number.

Coverage includes exact extraction assertions for every supported type and a
live-MySQL regression that checks the server-observed values at parameter counts
3, 8, 12, and 17, plus Date and Buffer contents. The checks compare the full
values; a constant non-null substitute cannot pass them.
