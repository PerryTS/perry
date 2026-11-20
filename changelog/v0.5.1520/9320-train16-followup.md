### Internal

- **Keeps `class_decl.rs` under the 2000-line file gate.** #9315 landed on a
  file already at 1989 lines. The non-computed member registration and
  `Symbol.iterator` wrapper helpers move to a sibling module unchanged.

- **Classifies two order side-tables in the root-holder inventory.**
  `CLASS_SYMBOL_MEMBER_ORDERS` is keyed by `SymbolHeader::id` — a stable id read
  once at registration, not a heap address, so it needs no re-key when a Symbol
  is evacuated — with a `u32` order value. `CLASS_DYNAMIC_PROP_ORDER` holds
  owned Rust strings. Neither stores a JSValue, so neither is a GC root.

- **Drops unnecessary parens in the numeric-range header read**, which
  `-D warnings` treats as an error.
