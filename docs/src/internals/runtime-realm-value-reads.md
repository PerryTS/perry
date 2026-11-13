# Runtime realm value reads

A runtime intrinsic read declares its subject and target beside the call:

```rust
crate::realm_value_read! {
    "Date" => "Date.prototype";
    crate::object::builtin_prototype_value("Date")
}
```

The build producer in `perry-runtime-manifest/src/realm_reads.rs` projects
this pair into the subject row's `runtime_reads` fact. The target must name
a declared realm cell. Read facts describe value dependencies; they add no
births to logical reach.

A read selected by a runtime constructor name uses the table form:

```rust
crate::realm_value_read! {
    table = GLOBAL_THIS_BUILTIN_CONSTRUCTORS;
    crate::object::builtin_constructor_value(name)
}
```

The producer obtains names from the runtime's `builtin_constructors!`
declaration and joins them to declared realm subjects. Prototype reads use
the reference in each subject's declared `prototype` slot. The macro accepts
no wildcard subject or independently maintained constructor list.

The producer rejects calls to `builtin_prototype_value`,
`builtin_constructor_value`, `intrinsic_prototype_addr`,
`intrinsic_constructor_addr`, and `IntrinsicPrivateReadSite::read` outside
these declarations, reporting the source file and line. Raw accessor
implementations and test code are excluded from the projection.

`OwnPrivateReadSite` supplies optional reads of ordinary private state.
`IntrinsicPrivateReadSite` uses that same lookup for realm cells and checks
missing values on image-held receivers. In test, debug, or
`perry_diagnostics` builds, a missing intrinsic value aborts with its cell
name, for example `runtime value read of RegExp.prototype is unrooted`.
Constructor and prototype address reads use the same named diagnostic when
an image global is bound.

The macro expands to its expression. The checked guards emit no code in a
release build without `perry_diagnostics`.


Live receiver shape reads pass both the object address and the already-loaded
header word to the object shape helpers. Resolved words held by another owner
use the held-word helpers. The ordinary receiver path remains the row-bound
comparison; the cold path retains the address for image resolution. These
reads do not name realm members or introduce EXEC events.

Layout word ABI 5 reserves row bit 25 as `LAYOUT_ROW_PENDING`. Issued rows
are strictly below `1 << 25`; encoding rejects the cap and larger rows, and
the native static table is const-checked. Program layout ordinals validate
each row during compilation. Region construction checks its capacity once;
chunk exhaustion then prevents any dynamic row from reaching the cap.
Static publication relies on those bounds without repeating the row check.
The dynamic encoder retains its checked encoding. A pending word
keeps its band and kind and fails ordinary receiver admission. Image binding
rejects any layout word ABI version other than 5. The pending bit denotes an
unpublished image row and its first-observation trigger consumes it.

Image receiver observation uses the existing slab's cold receiver branch.
A pending header cannot fill a site word or a PIC token. Its first program
read publishes the compiler's row from immutable image keys, then clears
the header's pending bit. Collector reads supply no receiver and therefore
never publish; a pending cell contains no collectable edge. Publication
runs without GC suppression and checked builds reject a GC allocation in
that window. The rebuilt runtime archive must classify
`js_realm_touch_pending` as Leaf.

The archive analysis follows an erased HotKey resolver in the context of
its proven receiver values. Exact address relocations and control-flow
reaching definitions identify the values; each declaration's immutable
resolver relocation supplies its initializer. Unknown receivers retain the
conservative target union. Ordinary callees and actual initializers remain
in the graph. A drop-glue call is omitted only when all control-flow
successors unconditionally reach `std::process::abort`; a returning,
unknown, or looping successor retains the edge. These are analysis rules,
not TLS- or dropped-type exemptions.

Builtin family observation enters one materialize point. A plain Intrinsics
entry naming a cell with LAYOUT_ROW_PENDING chooses BIND: publish only that
family's rows, clear its pending words, then run family_link. Other realms
use the same point's existing birth arm. Bound-only readers hide pending
cells. Constructor headers carry family identity; other image descriptors
carry one family byte in their padding, without changing descriptor size.
Object, TypedArray, iterator, and function-tower ensures enter the same
point. An observation before globalThis seeds only the existing Intrinsics
entry's plain addresses from image facts; it publishes no row and creates
no global. Checked births reject held families. The two arms share every
external root and prototype-identity write, while member creation and seal
bytes belong to family_cells.
