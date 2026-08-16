**Split `gc/layout.rs` and `codegen/artifacts.rs` back under the 2000-line file cap** (#8212). #8204's header shrink pushed `crates/perry-runtime/src/gc/layout.rs` to 2110 lines and `crates/perry-codegen/src/codegen/artifacts.rs` — which had been sitting at exactly 2000 — to 2005, turning the required `lint` context red on `main` for every open PR. Pure code moves, no behaviour change:

- the typed-shape layout installation protocol (`TypedShapeProof`, `init_typed_shape_layout`, `install_typed_shape_layout_slow`, `js_gc_init_typed_shape_layout`, `js_gc_declare_typed_shape_layout`) moves from `gc/layout.rs` into the new `gc/layout/typed_shape.rs` (2110 → 1778), next to the existing `layout/slot_mask.rs` split; the two extern "C" entry points keep their `crate::gc::` paths via an explicit named re-export;
- `synthesized_ctor_param_count` moves from `codegen/artifacts.rs` into the new `codegen/ctor_arity.rs` (2005 → 1930);
- `scripts/shape_descriptor_census_baseline.json` is refreshed mechanically for the one `keys_array` callsite whose file path changed (`raw_member_files` 65 → 66, same site and count).

Validated as relocation-only: zero-warning rebuild of both crates; `perry-runtime --lib` 2522/0/4; full `perry-codegen --no-fail-fast` failure set byte-identical by name to `origin/main` (1493/9 both); `perry --bin perry` 987/0; all GC script gates green.
