Lay the foundation for the object-header work (A1) as a no-op on main: every
program compiles and runs exactly as before. `GC_TYPE_OBJECT` stays 2 and no
second object type exists yet.

Add the object-header access API (`object/header.rs`: `MetaObj`,
`object_type`, `object_slot_at`, `object_meta_if_present`) and the
`gc_type_is_object` / `object_has_meta_word` predicates, which both answer
`t == GC_TYPE_OBJECT` today. Route the 45 meta-record creation sites through the
typed funnel `object_meta_ensure(MetaObj, MetaReason)`; `MetaReason` has one
variant per creation reason and `excluded_by()` names, without a wildcard, the
whole-program fact that keeps a meta-less family away from it. Sites that
cannot yet prove their owner is meta-form are marked `A1-UNPROVEN`.

Layout keys carry their slot's absolute byte offset (`PerryLayoutKey.off`, in
existing padding; emitted for the coming reader migration), and
`PerryLayoutDesc.gc_header_word` carries the idle GcHeader image of a layout
birth, emitted by codegen and pinned by const-asserts against `target_layout`.
`perry_alloc_layout` stores that image instead of composing it. The descriptor
is padded to 128 bytes so the descriptor-section index stays a shift (any other
size makes startup pay a longer division).

Add `scripts/check_object_header_access.sh` (count mode), the exact per-function
Ir diff tool `scripts/a1/irdiff.sh`, and the subprocess death-test harness with
`a1_internal_error` (a test proves the helper fails when the child survives).
