The link now removes every compiled function nothing live references, with no
analysis. Three things used to keep every function of a linked module: its
functions and constants shared one input section each (function and data
sections were off), every function's metadata sat in a per-module owner table
that module init registered, and the GC map was one retained directory per
module. Native objects now have one input section per function and per global
(`-ffunction-sections -fdata-sections` through a two-field adapter beside the
wasm one), JS bodies keep one section each inside `perry_js_text`, each body's
metadata record and GC map entry are tied to the body's own section (ELF
`SHF_LINK_ORDER`, Mach-O `live_support`, COFF associative COMDAT), and readers
walk the linker-bounded record section, which only holds survivors. The owner
keeps its identity and overrides and names its image's records with a
self-relative offset, so module init is unchanged. GC map v9 has one 20-byte
entry and one records blob per function, carrying the version in each entry.
wasm has no tied sections in wasm-ld, so there every record stays, as before.

Fastify inject is 3.0% smaller and Richards 3.1%; startup Ir does not increase.
Tests cover the record and entry formats, the unit split, the COFF COMDAT tie,
image copies for unloadable images, and a shared object's v9 map.
