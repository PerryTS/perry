| probe | Perry instr/call | Node | ratio | top runtime entries (share of the Perry profile) |
|---|---:|---:|---:|---|
| `f01_inline_arith` | 38 | 49 | 0.8× | `<inline JS>` 98.0%; `js_parse_int` 1.8%; `<no JS frame>` 0.2% |
| `f02_direct_call` | 86 | 48 | 1.8× | `<inline JS>` 99.9%; `<no JS frame>` 0.1% |
| `f03_closure_const` | 299 | 27 | 11.3× | `js_implicit_this_set` 43.1%; `<inline JS>` 23.7%; `fmod` 20.0% |
| `f04_class_method` | 51 | 32 | 1.6× | `<inline JS>` 98.5%; `object::field_set_by_name::tail::set_field_by_name_object_tail` 1.4%; `<no JS frame>` 0.1% |
| `f05_objlit_method_this` | 4222 | 32 | 133.5× | `js_typed_feedback_native_call_method_by_id` 75.4%; `js_object_get_field_by_name_f64` 16.2%; `js_class_field_get_ic` 5.3% |
| `f06_fn_call` | 3702 | 100 | 36.9× | `js_typed_feedback_native_call_method_by_id` 94.0%; `js_dynamic_mod` 2.3%; `js_set_call_location` 2.2% |
| `f07_private_method` | 934 | 36 | 26.0× | `js_private_method_call` 75.4%; `js_set_call_location` 11.6%; `js_private_method_guard` 3.9% |
| `f08_proto_fn_method` | 9551 | 49 | 194.4× | `js_typed_feedback_native_call_method_by_id` 97.1%; `<inline JS>` 0.8%; `<no JS frame>` 0.8% |
| `f09_untyped_receiver_method` | 160 | 39 | 4.1× | `<inline JS>` 99.5%; `<no JS frame>` 0.5% |
| `f10_arguments` | 509 | 231 | 2.2× | `js_arguments_bundle_index_get` 43.8%; `js_array_length` 30.1%; `<no JS frame>` 11.5% |
| `f11_class_getter` | 484 | 30 | 16.3× | `object::field_get_set::ic_miss::get_field_ic_miss_impl` 52.6%; `js_inherited_read_cache_hit_f64` 29.7%; `<inline JS>` 8.7% |
| `f12_imported_obj_read_escaped` | 1839 | 50 | 36.9× | `js_object_get_field_by_name_f64` 76.6%; `js_class_field_get_ic` 16.1%; `<inline JS>` 7.2% |
| `f13_imported_obj_read_local` | 90 | 66 | 1.4× | `<inline JS>` 99.7%; `js_mark_entry_module_esm` 0.1%; `<no JS frame>` 0.1% |
| `f14_absent_key_read` | 8728 | 47 | 186.5× | `object::field_get_set::ic_miss::get_field_ic_miss_impl` 96.7%; `js_inherited_read_cache_hit_f64` 1.7%; `<inline JS>` 0.6% |
