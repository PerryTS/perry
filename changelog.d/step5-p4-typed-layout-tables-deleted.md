- runtime/codegen: the typed-layout descriptor tables are gone. `SHAPE_LAYOUTS`,
  `TYPED_LAYOUTS`, the construction memo (`gc/shape_install.rs`) and the
  `js_gc_init_typed_shape_layout` / `js_gc_declare_typed_shape_layout` entry
  points are deleted with their codegen calls (class `new`, object literals,
  scalar-receiver materialization), declarations, gc-effects and wasm ABI rows.
  A class instance's lanes come from its ShapeId (minted with the birth rep), and
  nothing sets the typed-layout-intact bit on an object; the constructor-prologue
  raw-f64 store path no longer tests it. `HotTls` loses three slots
  (`implicit_this` 128 -> 104, `agent_ptrs` 136 -> 112) (charter step 5, P4 flip).
