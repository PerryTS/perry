Exporting an asymmetric KeyObject with `format: "jwk"` went through the
PEM-backed surrogate, which cannot express a JWK. It now dispatches to the
webcrypto encoder and returns a plain JWK object.
