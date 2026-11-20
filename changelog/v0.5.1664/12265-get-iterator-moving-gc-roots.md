Keep GetIterator inputs current across allocating type probes, symbol getters, factory rebinding, and async-iterator fallback lookup. Root the input before any probe and refresh it from a runtime handle without changing property lookup or callback order.

Restore ordinary-source coverage in the raw iterator adapter relocation regression and add deterministic copying-nursery witnesses for getter-to-factory receiver identity and sync/async fallback lookup order. Correct the iterator-helper fragment filename to reference PR #12264.
