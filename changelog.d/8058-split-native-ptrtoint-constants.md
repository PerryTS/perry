### Fix relocatable constants in split native codegen

Production-sized modules using split native codegen now materialize LLVM
`ptrtoint` constant operands for function and global references instead of
misparsing them as integer literals. Module-init wrappers can therefore pass
their `__init_body` function pointer through the runtime ABI while retaining a
real relocation in each independently emitted and partially linked object.
