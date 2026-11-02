Fix Greek final-sigma casing in `String.prototype.toLowerCase()`: a capital
sigma preceded by a cased letter and not followed by one now becomes `ς`,
skipping Unicode case-ignorable characters on either side. Whole-string
lowercasing preserves this context while bounded decoding retains lone
surrogates and malformed payload boundaries. The ASCII fast path remains
unchanged.
