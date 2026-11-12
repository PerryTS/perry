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
