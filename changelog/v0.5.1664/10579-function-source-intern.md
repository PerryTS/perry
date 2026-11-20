### Performance

- **Nested `Function.prototype.toString` source is interned at tsc scale
  (#10574).** Codegen already shared overlapping function bodies via
  `SourcePool`, but intern turned *off* when unique-string lengths summed
  past 8 MiB. A CJS bundle like `typescript/lib/_tsc.js` is ~24 MB of
  nested slices of a ~6 MB module, so `__cstring` kept one copy per
  function (24.3 MB, 28% of an 86 MB tsc binary). Over-budget modules now
  still share into the longest parent (the CJS factory / module wrapper).
  `fn.toString()` is byte-identical. The remaining unique source (~6 MB)
  can be dropped with `--function-source=header` /
  `PERRY_FUNCTION_SOURCE=header`, which stores
  `function <name>(<params>) { /* source elided */ }` instead of the body
  — enough for name extraction and parameter-name DI, not enough to
  reconstruct bodies. Full interned source stays the default.
