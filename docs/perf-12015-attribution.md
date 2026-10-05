# Object metadata and enumeration attribution (#12015)

Base and fetched origin/main: `2ebeca0e8d9fe6e47b9d30a478bbb7a122c9bcbd`.

Issue [#12015](https://github.com/PerryTS/perry/issues/12015) was read without changes. The owner charter requires the shape to own receiver facts; a call site may memoize a ShapeId and slot, validated on every use. No new registry, side table, latch, or property-name special case is admissible. RegExp, object/private*, statepoint lowering, and Function.call/apply/bind implementations are excluded.

## Retained attribution (historical evidence)

The supplied attribution is from `aef2f031910b7df14b80e893e4471c5439c9f909`, not this base. Three cycle-weighted recordings per workload were pooled. Percentages below are the self-time **charged to this bucket**, extracted from every function’s `all_mapping_parts_json`; inclusive totals and a function’s other bucket charges are excluded. Only the six functions TSV and six analysis JSON files were extracted. The equal-workload bucket mean was 7.61%. These are opportunity shares, not predicted savings.

| Program | Bucket share | Leading functions (bucket self-time) |
|---|---:|---|

| commander | 4.679% | `class_registry::class_meta::is_anon_shape_class_id` 0.393%; `exotic_expando::expando_clear_on_alloc` 0.270%; `prop_plan::read_plan_lookup` 0.225%; `shapes::shape_descriptor_intern_with_special_mode` 0.224%; `dictionary::is_dictionary` 0.203% |

| fastify | 8.566% | `class_registry::class_meta::is_anon_shape_class_id` 0.569%; `key_attrs::object_key_entry_filtered` 0.451%; `descriptor_state::class_instance_set_may_intercept` 0.371%; `iterator_prototypes::note_iterator_prototype_exposed` 0.333%; `shapes::shape_descriptor_intern_with_special_mode` 0.318% |

| qsparse | 7.596% | `exotic_expando::expando_lookup` 0.659%; `class_registry::class_meta::is_anon_shape_class_id` 0.544%; `native_module::callable_exports::bound_native_callable_module_and_method` 0.369%; `key_attrs::object_key_entry_filtered` 0.288%; `shapes::stamp_object_shape_id_with_carrier_note` 0.267% |

| qsstr | 8.979% | `static_shapes::finalized_constfn_facts` 1.211%; `shapes::shape_descriptor_intern_with_special_mode` 0.852%; `class_registry::parent_static::is_class_object_ptr` 0.841%; `native_module::callable_exports::bound_native_callable_module_and_method` 0.547%; `shapes::birth_stamp_object_shape` 0.474% |

| tsc | 6.654% | `shapes::shape_descriptor_intern_with_special_mode` 0.597%; `class_registry::class_meta::is_anon_shape_class_id` 0.403%; `key_attrs::object_key_entry_filtered` 0.356%; `field_rep_store::checked_slot_bits` 0.326%; `canonical_keys::probe_node` 0.224% |

| zod5k | 9.197% | `shapes::shape_descriptor_intern_with_special_mode` 0.906%; `class_registry::class_meta::is_anon_shape_class_id` 0.751%; `field_rep_store::checked_slot_bits` 0.579%; `live_slots::object_live_slot_count` 0.446%; `native_module::bound_native_method_length` 0.403% |


The bucket includes shape creation and receiver metadata used by reads and stores, not just Object.keys. In qs stringify, `finalized_constfn_facts` and its callers dominate; optimizing only a keys loop would miss that mechanism.

## Retained compiled sites

Offsets refer to the historical binaries and must be remapped in fresh profiles. The site share is disjoint bucket leaf time at the nearest compiled caller, not an inclusive cost.

| Program | Leading compiled sites (bucket whole-program share) |
|---|---|

| commander | `node_modules_commander_lib_command_js__Command_constructor+0xa6` 0.547%; `perry_method_node_modules_commander_lib_command_js__Command__addOption+0x2253` 0.459%; `node_modules_commander_lib_command_js__Command_constructor+0x553b` 0.360% |

| fastify | `perry_closure_node_modules_light_my_request_lib_request_js__17+0x467` 1.014%; `perry_closure_node_modules_light_my_request_lib_response_js__43+0x1dc` 0.630%; `perry_closure_node_modules_light_my_request_lib_request_js__17+0x7abe` 0.431% |

| qsparse | `perry_closure_node_modules_qs_lib_utils_js__20+0x6a0` 1.106%; `perry_closure_node_modules_qs_lib_utils_js__30+0xd1` 0.947%; `perry_closure_node_modules_qs_lib_parse_js__18+0x6997` 0.695% |

| qsstr | `perry_closure_node_modules_side_channel_weakmap_index_js__16+0x60e` 1.912%; `perry_closure_node_modules_side_channel_weakmap_index_js__16+0x40a` 1.515%; `perry_closure_node_modules_side_channel_index_js__11+0x4d7` 1.452% |

| tsc | `perry_closure_node_modules_typescript_lib_typescript_js__5860+0x22cc` 0.296%; `perry_closure_node_modules_typescript_lib_typescript_js__3109+0x1c0` 0.279%; `perry_closure_node_modules_typescript_lib_typescript_js__5246+0x85` 0.274% |

| zod5k | `perry_method_node_modules_zod_lib_index_mjs__ZodString___parse+0x10df` 0.567%; `perry_method_node_modules_zod_lib_index_mjs__ZodString___parse+0x10b8` 0.455%; `perry_closure_node_modules_zod_lib_index_mjs__39+0x1325` 0.413% |


## Shape facts and site memos

- Own key order, logical count, attributes, holes, receiver kind, prototype identity, field representation and ConstFn body identity belong on the shape. Existing `ShapeRecord` and its record-owned extension already carry most of these facts. Enumeration must use the logical prefix count, never the shared backing’s length.

- A descriptor-free/data-only proof is an attribute-summary fact, not an owner-table query. Once proved, values and entries can load slots directly without rebuilding strings or doing named lookups. Getter/proxy paths must retain snapshot and per-key descriptor rechecks because user code can change later keys.

- An ordered enumerable-slot list, if justified, belongs to the shape record. It must not add a per-object registry or untraced GC pointer. Measure construction and RSS costs before choosing a retained list over existing key storage.

- A repeated method read may remember `(ShapeId, slot)` at the call site, then read the current slot after the shape compare. Captured closures are receiver values; caching the closure itself would use another receiver’s captures. Bind’s implementation remains outside this lane.

- Completed ConstFn promotion must prove the source shape and the current function slots. Static names are not a substitute for shape authority. Repeated parsing of packed key names is a candidate only if fresh profiles support it and the replacement establishes the same complete facts.

## Parameter-receiver micro-cases

`benchmarks/object_metadata_12015/` contains five mechanisms: keys with a non-enumerable property, data-only values/entries, descriptor/name reflection, for-in with computed reads, and spread/assign. Operations accept `receiver: any`; literals exist only in the caller. Each row counts one loop body as one operation, including both APIs where named. Instructions/op must be measured against pinned Node 26.5.1, with an empty-loop/startup arm accounted for consistently.

## Fresh measurement and implementation status

Pending: symbol-bearing fresh main profiles, source-site mapping, micro measurements, supported fixes, sabotage and gap tests, separate-target interleaved n=5 instruction/RSS medians, baseline failure lists, and lint/fmt/root-dominance validation. Historical evidence above must not be mistaken for fresh validation. No performance claim is made at this milestone.
