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
births to logical reach. The close adds each selected reader's targets as
held names without making their bodies WHOLE.

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
the agent has an image global, including before its first materialization.

The macro expands to its expression. The checked guards emit no code in a
release build without `perry_diagnostics`.


Live receiver shape reads pass both the object address and the already-loaded
header word to the object shape helpers. Resolved words held by another owner
use the held-word helpers. The ordinary receiver path remains the row-bound
comparison; the cold path retains the address for image resolution. These
reads do not name realm members or introduce EXEC events.

Layout words reserve row bit 25 as `LAYOUT_ROW_PENDING`. Issued rows
are strictly below `1 << 25`; encoding rejects the cap and larger rows, and
the native static table is const-checked. Program layout ordinals validate
each row during compilation. Region construction checks its capacity once;
chunk exhaustion then prevents any dynamic row from reaching the cap.
Static publication relies on those bounds without repeating the row check.
The dynamic encoder retains its checked encoding. A pending word
keeps its band and kind and fails ordinary receiver admission. Image binding
uses program layout-image ABI 6. Its table carries the Global address and
contiguous intrinsic-run bounds after the existing lazy-row fields; the
layout-word bit allocation is unchanged. The pending bit denotes an
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

The compiler retains the logical selected RealmPlan independently of physical
image activation. ImagePlan emits cells only when the actual lowered EXEC
facts include the realm-creating operation. A program such as hello that
never performs that operation emits no realm cells or Global address, while
its logical prototype chains and required runtime descriptors remain intact.
No realm bind call is emitted in main.

On first Global use, the same getter's cold arm reads its immutable program
table. An image arm publishes the Global-owned rows and root words and seeds
held Intrinsics halves in one contiguous-run walk. Other families remain
pending. A realm without an image keeps runtime allocation and population.
Workers and vm/eval realms do not acquire pending image cells.

A native namespace owns the source birth graph recorded beside its birth:
its constructor, prototype, members and captured dispatch receiver. Such
native-owned graphs use their existing birth arm when native image membership
is unavailable. This classification does not add R or EXEC facts. Shared
values can declare an actual source birth owner independently of every
holder whose slot aliases them; a slot alias does not transfer ownership.

Private intrinsic keys carry their target role. The shared ABI decoder lets
emission store a held image target in private storage without observing its
pending row. If the close holds no target, the slot remains undefined and a
checked runtime value read aborts by name. Namespace seals and prototype
identity metadata likewise link their existing per-agent words on the owning
family's observation. Birth and image publication share that link body;
optional contract snapshots read canonical rows without allocating managed
own-name arrays.


The image stores only `singleton` birth identities. A source declaration with
`birth_domain = instance` describes the callable body made by each owner
allocation: its logical reach and EXEC effects remain, while its existing runtime
birth supplies each identity and capture. Such a body is never an image cell.
Optional hook members declare `installed_by` at their source birth. The logical
member rule and physical plan follow that member edge only when the installer
executes; registering its pointer does not execute its targets. An explicitly
owned body or direct body EXEC retains all its declared births and effects. The compiler obtains those
EXEC inputs from the same instructions and call lists it emits for the installer.


Image and runtime cells of a declared symbol share the identity in
`SymbolHeader.id`. Ordinary fixed and overflow keys, canonical key lists,
strict equality, and Map/Set use the same `js_symbol_equals` predicate.
Collection hashes use that identity as well. Distinct symbols retain distinct
IDs even when their descriptions match; no address adoption or symbol registry
is needed to join an image key with its native birth.


The restricted descriptor oracle consumes the compiler's selected RealmPlan
cells and WHOLE identities from the reach diagnostic. It projects Global and
STRUCTURE rows by those exact member facts before comparing descriptors,
order, symbol identities and prototype edges. A chain-held identity does not
require a Global property. Missing selected slots or prototypes fail the oracle.
The dumper obtains the Global value through the existing GlobalThisExpr
materialization and reaches intrinsic roots along the fixture's held paths;
it does not retain a Global alias to make reflection work.
