Unify generated and Rust runtime function layout under one recorded ELF order.
A Linux perf companion records hot runtime/stdlib symbols without instrumenting
the runtime and groups aliases of each existing optimized body. Ordered Linux
executables align and back only their rounded hot
text prefix with transparent huge pages, trying shared file-backed collapse
before a bounded anonymous remap. Normal links and non-Linux startup code stay
unchanged; all functions retain the same optimization and execution semantics.
