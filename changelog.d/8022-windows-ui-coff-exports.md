### Windows UI links current Rust codegen archives again

Windows `perry/ui` builds no longer lose every public `perry_ui_*` export
while trimming duplicate COFF archive members. The trimmer now recognizes the
opaque `.rcgu.o` member names emitted by current Rust toolchains, extracts all
of the UI crate's codegen units, and continues to exclude non-codegen allocator
members.
