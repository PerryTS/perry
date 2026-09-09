Update the external TypeScript provider's direct parser to
`swc_ecma_parser` 32, matching Perry's workspace parser while preserving the
existing Windows dependency selections. The provider's current SWC bundler
and transform stack continues to carry parser 29 transitively.
