Fixed module-level symbol collisions when schema-directed JSON parsing, V8
interop, or Node submodule literals are emitted in more than one body for the
same source function. Specialised and boxed bodies now allocate distinct
private rodata names instead of producing LLVM global redefinition errors.
