Reuse the rooted input string's UTF-16 length when constructing a dominant,
large unescaped JSON string value. Require an exact whole-source match and at
most 512 ASCII bytes outside the token; malformed tails, slices, escaped
strings and other shapes retain the existing counter. Output allocation,
copying and reclamation accounting remain unchanged.

Cover byte-count equivalence across malformed inputs, astral characters,
surrogates, truncated tails and vector boundaries, and reject mismatched input
owners and ranges. Performance acceptance is recorded separately from these
correctness checks.
