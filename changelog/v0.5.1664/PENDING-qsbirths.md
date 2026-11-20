### Performance

- Birth completed closure-containing literals directly on their seeded ConstFn shape after validating the current values. This removes intermediate Any births, checked field stores and post-construction promotion for these literals, while preserving fresh closures and rooting evaluation inputs.
- Reuse validated scalar birth-shape sites for WeakMap and WeakSet construction. Every use checks the live shape record; misses and retired shapes use the existing interner.
