The compiler now emits the descriptor and key list of every runtime object
layout the program's realm reaches (R), and the runtime no longer carries a
catalog of its own. Each layout in R is a standalone descriptor under its
direct symbol `perry_runtime_layout_<NAME>`, with its fixed native layout word,
its keys, index, prototype record, gc plan and (for string-keyed layouts) its
link-time key list in `perry_realm`. Layouts with a symbol key name the
runtime's own symbol atoms and build their key list at first birth until the
realm owns its symbols. Nothing is emitted for a layout outside R.

Every runtime birth reaches its descriptor through a weak reference to that
symbol and aborts with the layout's name when the program's realm does not
reach it. The positional descriptor lookup over the `perry_layouts` section,
wasm's runtime layout inventory, the const image key-list machinery and the
unused `CJS_MODULE_RECORD` layout are removed; its ABI row stays reserved so
no other native word moves. Runtime tests, and test crates that link the
runtime without a compiled program (feature `test-layout-descriptors`, enabled
only from dev-dependencies), expand the same declarations into test-only
definitions.

A module now keeps one definition of each content-named coalesced global
(image strings and atoms), so independent emitters of the entry module can
name the same key. Tests check every emitted descriptor cell and key entry
against the declared facts and the compiler's word rule against the runtime's
word table; `scripts/realm_wgraph_gate.py` proves on a linked hello that only
R's descriptors are present, each tied to its own key list, and that no
test-only definition is linked.
