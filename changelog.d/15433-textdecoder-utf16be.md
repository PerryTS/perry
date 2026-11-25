`new TextDecoder("utf-16be")` and its label `unicodefffe` decode big-endian UTF-16 instead of throwing `ERR_ENCODING_NOT_SUPPORTED`. Both UTF-16 decoders throw on malformed input with `fatal: true`.
