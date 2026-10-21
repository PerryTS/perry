Fix native compilation and cross-module linking for Unicode JavaScript class,
constructor, method, accessor, function and export names, including escaped
identifiers and default-exported named classes. Definitions and references now
share linker-safe symbol constructors and one injective UTF-8 byte encoder.
ASCII letters, digits and underscores keep their spelling; other bytes use
`.uHH` escapes. Dollar signs are escaped to preserve the compiler's reserved
generated-suffix namespace. Function/class reflection names remain source names.

Remove raw export-name compatibility aliases now that importers use the same
encoding. Module prefixes preserve existing ASCII path spellings and use the
same byte encoder for Unicode paths. Regression coverage includes two-module
Node parity, encoder round trips, GC sidecars, IR references and stack maps.
