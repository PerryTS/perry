**codegen/GC: stop GC-typing variable-box pointers and leaf-mark the audited capture/box accessors (#8132 direction 1)**

RS4GC's output scales as safepoints × live GC values, and on next\@16.3.0's bundled `jsonwebtoken` one webpack module factory concentrated 95% of the unit's 1.5M `gc.relocate`s: 5,536 statepoints × a mean of 259 live `addrspace(1)` values (`perry_closure_jsonwebtoken_js__227`). Dissecting the live sets showed two populations this change removes at the source, per the issue's "not modelling every value as a GC pointer where a proof exists":

- **Variable-box pointers are not GC pointers.** A boxed local's slot only ever holds a `js_box_alloc_bits`-family result (or the `TAG_UNDEFINED` sentinel): boxes are `std::alloc` allocations outside the GC heap, never moved, never freed (`BOX_REGISTRY` is monotonic), and the JSValue inside is traced by the registered `scan_box_roots_mut` scanner — the premise `scripts/gc_root_dominance_check.py`'s IMMOVABLE_SOURCES "box" probes already machine-check, and the one `expr/literals_vars.rs` already relies on to carry a box address across collecting calls. `emit_shadow_slot_bind_for_local` now skips boxed locals (`boxed_vars && !module_globals`, the exact test every store site uses), so their slots stay plain `alloca i64` and RS4GC never relocates them. On the fixture ~300 preallocated boxes were live across ~90% of the monolith's statepoints.

- **The audited capture/box accessors are leaf calls.** `js_closure_get/set_capture_bits` (+ `_ptr`), `js_box_set_bits`, `js_box_alloc_bits`, and the i32/bool box helpers are raw slot reads/writes plus already-admitted barrier/layout bookkeeping, or `std::alloc` allocation that cannot arm a Perry GC trigger. They join `GcCallEffect::CannotCollect` (and the checker's NONCOLLECTING, preserving the one-way containment). They were 2,168 of the monolith's 5,537 statepoint-forming calls. `js_box_get_bits` is deliberately **not** admitted: its TDZ arm allocates a ReferenceError before unwinding, and a test pins it to `Unknown`.

Measured on the #8132 fixture (stock `opt` 22.1.4, `function(mem2reg,sccp),rewrite-statepoints-for-gc` on unit0):

| metric | before | after |
|---|---|---|
| fn227 statepoints | 5,536 | 3,368 (−39%) |
| fn227 gc.relocate | 1,432,110 | 477,377 (−67%) |
| fn227 mean live values/statepoint | 258.7 | 141.7 |
| unit0 gc.relocate | 1,503,308 | 522,099 (−65%) |
| unit0 post-RS4GC IR | 412 MB | 161 MB (−61%) |

Tests: the boxed local's slot is asserted un-retyped beside an unboxed twin that still lowers `alloca ptr addrspace(1)` (discriminating in both directions), and a statepoint-rewrite probe asserts the audited accessors stay direct calls while an unaudited callee beside them is statepoint-wrapped.
