`KeyObject.export({ format: "jwk" })` on an RSA or P-256 key returns a plain
JWK object instead of the PEM-backed surrogate string, and P-256 JWKs list
`crv` after `y` as Node does.
