### Fixed

- **Moving GC no longer corrupts pointer locals of constructor bodies inlined
  into other frames** (#9081). Codegen splices constructor bodies inline at
  five sites — the `super(...)` parent-body inline in a derived constructor
  (`expr/this_super_call.rs`), the `new`-site own/inherited-ctor inlines
  (`lower_call/new.rs`), and `let_stmt`'s scalar-ctor variants — but the
  enclosing function's shadow-slot map was computed by
  `collect_pointer_typed_locals` over that function's OWN params and body, so
  a spliced body's pointer locals (and its ctor params, bound by
  `bind_inline_constructor_params`) landed in plain entry allocas the
  collector never rewrites. A moving minor between a local's store and a
  later use left the local holding the pre-move address.

  The reported shape: three.js 0.180.0 under `compilePackages` with a 1 MiB
  nursery. `RenderTarget`'s ctor body, spliced by the super() inline into the
  standalone `WebGLRenderTarget_constructor`, holds `const texture = new
  Texture(image)` across `this.textures = []` and the attachment loop; the
  retired-from-space `texture` then made `Texture.copy()` read
  `source.mipmaps` as undefined — `TypeError: Cannot read properties of
  undefined (reading 'slice')`. The from-space quarantine
  (`PERRY_GC_PROTECT_FROMSPACE=1`, depth 800) named the stale header deref
  inside the constructor, and the emitted IR showed the spliced `texture`
  alloca carried no root bind while the standalone `RenderTarget_constructor`
  rooted the same local at slot 4.

  Fix: `expr/shadow_slot.rs::root_inlined_ctor_pointer_locals`, called at all
  five splice sites before lowering the spliced body. It runs the same
  pointer-locals collector over the spliced params+body and extends the frame
  through `reserve_shadow_slot`, which grows the already-emitted frame in
  place on both root backends (native stack maps and shadow frames — the bug
  reproduced under both `PERRY_RS4GC` arms). Already-bound ctor params are
  bound immediately and re-bound on a repeated inline of the same
  constructor; module-unique local ids make the map extension alias-free, and
  nested `super()` chains recurse through the same site.

  Regression test `test_gap_gc_inlined_ctor_body_locals_rooting.ts`
  (registered in the gc-repsel corpus) runs the seeded every-poll evacuating
  schedule with the quarantine armed: the unfixed compiler SIGSEGVs at minor
  #0; fixed, it is byte-exact with the oracle on both backends. The original
  three.js reproduction passes on default heap, forced 1 MiB nursery,
  quarantine, and the `PERRY_RS4GC=0` build, with `PERRY_GC_DIAG` confirming
  live copying collections. Also relocates `is_global_this_value` from
  `let_stmt.rs` to `let_stmt_facts.rs` (pure move, 2000-line cap).
