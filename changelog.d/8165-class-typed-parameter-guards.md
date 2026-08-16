### Performance

- Admit class-typed parameters into the #8094 guarded specialization path. A
  class descriptor now carries the class id — giving
  `param_type_guard.rs`'s `class_chain_reaches` branch its first caller, which
  codegen had never reached — plus every declared field on the inheritance
  chain, validated by name. A class-annotated parameter recovers the same
  lowering an interface-annotated one already got (`js_dynamic_string_or_
  number_add` becomes a string concat), while a structurally identical object
  literal fails the identity check and takes the generic fallback (#8099).

  The refusal this replaces rested on a stale claim that compact class
  instances carry no `keys_array`. They do —
  `object_alloc_class_inline_keys_impl` installs a per-class array built once
  at module init — so the same by-name field validation that serves interfaces
  serves classes. The stale note on `ObjectHeader::keys_array` is corrected
  too.

  Identity **without** the field types was implemented, measured and reverted:
  the emitted clone came out structurally identical to the `$generic` sibling
  it routes around (same line count, same call multiset), because a
  class-annotated receiver already reaches the class-field guard path without
  any parameter evidence. It bought nothing and cost one `js_param_type_guard`
  call per invocation — `tree` 1.089 s → 1.646 s (+51%) and `tree_wide`
  1.775 s → 2.304 s (+30%), best-of-5 on the quiet M1 mini. That also answers
  the `tree` row #8099 was filed about: the hot recursive walker is refused by
  #8094's aliasing rule, not by the class refusal, and the only descriptor
  cheap enough to admit there is the one that buys nothing.

  Cost stays bounded by the existing rule rather than a new one: a
  field-bearing descriptor claims heap contents, so a reference-typed
  parameter carrying one is refused in any body containing a call. A recursive
  class (`Tree.left: Tree`) therefore cannot be guarded inside the recursive
  walker that would make its validation O(nodes x depth).

  Validated: the 19-program specialization corpus emits **identical LLVM IR**
  before and after, so nothing already-specializing moved; the extended
  `test_gap_specabi_ordinary_param_guards` fixture is byte-exact against node
  `v26.5.1`, including under `PERRY_GC_ZEAL=1 PERRY_GC_PROTECT_FROMSPACE=1`
  (40 copying minors, 40 from-space protections — the instrument was live);
  and `cargo test -p perry-codegen` adds no failure against a clean-`main`
  baseline run.
