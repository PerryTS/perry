Fix asynchronous zlib one-shot codecs rejecting `(buffer, options, callback)`, including an explicit `undefined` options argument. Direct calls and captured/promisified functions now preserve all three arguments in both bundled and external zlib implementations, while retaining `(buffer, callback)` support.

Gzip, deflate, and deflateRaw now carry the validated compression level into the codec operation, using the same flate2 level mapping as their synchronous counterparts (`-1` selects the default). Existing backend limitations for other codec options are unchanged; this does not promise byte-identical compressed output to Node. Callback roots survive options getters and async-hooks initialization, and the bundled codec worker receives only owned Rust data.

Regression coverage checks all eleven codecs, callback timing/validation, optional argument forms, direct calls, promisify, level-dependent output, and round trips.
