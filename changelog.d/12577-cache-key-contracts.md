Fix per-module object-cache reuse across contract-check and ordinary builds
(#12577). The existing key now includes the raw `PERRY_CHECK_CONTRACTS`,
`PERRY_METHOD_CERT_SABOTAGE`, `PERRY_B4_SABOTAGE`, and
`PERRY_ASSERT_CAPTURE_CELLS` values, so changing witnesses or proof negative
controls cannot reuse objects emitted under another setting. This removes
inconsistent environment identity between the build and object cache for
these inputs without adding another cache or codegen path.

Regression coverage compiles a pristine builtin fixture with contracts and
then without them in one cache directory, checks the linked object's undefined
symbols for any contract entry, verifies output, and checks reuse in both
directions. Unit tests cover contract-mode spellings and sabotage selectors.
