### Fixed

- Propagate the producer-selected root backend into split LLVM codegen workers so native-root units cannot skip statepoint rewriting and compact GC-map emission (#8070).
