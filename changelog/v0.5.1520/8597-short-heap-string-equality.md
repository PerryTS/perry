Improved equality checks between statically proven strings by comparing heap-string
lengths and up to three payload bytes inline before falling back to the full runtime
helper. This targets the short identifiers used by tree-walking interpreters: retired
instructions fell 2.41% for `interp` and 1.62% for `iso_miss`, with RSS effectively
unchanged. Generic-key comparisons retain the smaller existing dispatch.
