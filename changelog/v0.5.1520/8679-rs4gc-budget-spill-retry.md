Fixed large native functions whose statepoint rewrite crossed the LLVM
instruction budget but fell below the root-spill estimate threshold. Perry now
re-lowers only those functions with precise shadow-frame roots and retries the
same optimization pipeline instead of refusing the whole codegen unit.
