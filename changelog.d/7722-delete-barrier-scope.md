### repsel: document and pin why the `delete` shape barrier's per-module scope is sound for proven-`this` (#7143)

Investigated #7143 ("the Phase 5a `delete` shape barrier is module-scoped,
but a proven-`this` receiver is aliased across modules by construction").
Confirmed the asymmetry the issue describes: `ModuleDispatchFacts::has_shape_barrier_sites()`
is computed per module (`collect_module_dispatch_facts`), and Phase 5a's
proven-`this` admission (`collectors/proven_this.rs::method_proven_this`)
consults only its OWN module's copy — a `delete`/`Reflect.deleteProperty` on
a class instance in a module that imports the class, rather than declaring
it, sets no flag the declaring module's admission decision can see.

**No miscompile exists.** Built a two-module reproducer
(`test-files/test_issue_7143_delete_barrier_cross_module.ts` +
`test-files/fixtures/issue_7143_pkg/shared.ts`) matching the issue's own
suggested shape — module A declares `class C { a; b; c }` plus a method
reading `this.c`, admits a `$pshape` clone since it contains no `delete`
itself; module B holds an instance A handed it, deletes `b` off it (which
relocates `c`'s packed slot via `perry-runtime`'s keys-array compaction),
then calls back into module A, which dispatches `inst.readC()` on the
now-mutated object. The compiled binary's output matches
`node --experimental-strip-types` exactly (`3`, `3`, `3`), and the
`--trace llvm` IR shows why: EVERY routing site that can call a `$pshape`
clone independently re-derives soundness at the point it matters, rather
than trusting the per-module admission fact —

- `method_direct.fast` (`lower_call/method_override.rs`) sits behind
  `js_typed_feedback_method_direct_call_guard` / `js_method_direct_shape_guard`,
  whose contract includes a raw pointer compare of the receiver's live
  `keys_array` against the class's canonical keys token. `delete`'s only
  code path for a class instance with a keys array
  (`perry-runtime/src/object/delete_rest.rs::js_object_delete_field`, shared
  by `Reflect.deleteProperty`) always clones a FRESH keys array, so this
  compare can never pass on a post-delete receiver, from any module.
- The Phase 3b guard-free `Ptr<Shape>` receiver arm needs no runtime check:
  its containment proof (`collectors/ptr_shape.rs` rule 2) already rules out
  any alias to the object existing anywhere, so there is nothing for a
  cross-module `delete` to reach through.
- The #7142 class-id dispatch-tower case
  (`dynamic_dispatch.rs::emit_tower_pshape_call`) already carries its own
  explicit keys-token re-check, added specifically for this reason — its doc
  comment already cited #7143 by number.

Landed as a documentation + regression-test PR, not a bug fix: added a
"`delete` is aliased across modules by construction" section to
`collectors/proven_this.rs` stating this invariant explicitly (module-wide
barrier facts are a cost heuristic for Phase 5a, never the correctness
mechanism — a future 4th routing site must independently re-derive a
dominating runtime check or genuine containment), a cross-reference from
`collectors/ptr_shape.rs`'s module-wide barrier rule, a new
`guarded_pshape_call_site_is_preceded_by_a_keys_token_guard` IR ratchet in
`proven_this_routing_tests.rs` pinning the `method_direct.fast` guard
dominance (mirroring the existing tower ratchet), and the two-file
reproducer above as a permanent parity-suite fixture.
