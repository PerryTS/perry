### Fixed

`Sign.sign(key, encoding)` now returns the requested encoded string, and
`Verify.verify(key, signature, encoding)` decodes string signatures using the
same encoding rules as Node and Buffer.
