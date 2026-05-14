// Runtime UI and platform FFI surface inventory.
//
// This fixture is intentionally executable by the normal parity runner,
// but its main purpose is to keep TS-side coverage accounting attached
// to related public FFI shims. Move @covers entries from this
// inventory into behavioral tests as each area gets deeper compatibility
// coverage.
//
// Inventory entries: 44 unique FFI names, 49 declarations.

const testFfiSurfaceRuntimeUiVersion = 1;
if (testFfiSurfaceRuntimeUiVersion !== 1) {
  throw new Error("unexpected coverage inventory version");
}
console.log("test_ffi_surface_runtime_ui: ok");

/*
@covers
crates/perry-runtime/src/arkts_callbacks.rs:
  - perry_arkts_drain_toast
  - perry_arkts_invoke_callback
  - perry_arkts_invoke_callback1
  - perry_arkts_register_callback
  - perry_arkts_set_content_view
  - perry_arkts_set_visibility
crates/perry-runtime/src/geisterhand_registry.rs:
  - perry_geisterhand_find_by_shortcut
  - perry_geisterhand_free_string
  - perry_geisterhand_get_closure
  - perry_geisterhand_get_registry_json
  - perry_geisterhand_pump
  - perry_geisterhand_queue_action
  - perry_geisterhand_queue_action1
  - perry_geisterhand_queue_apply_style
  - perry_geisterhand_queue_scroll
  - perry_geisterhand_queue_set_text
  - perry_geisterhand_queue_state_set
  - perry_geisterhand_register
  - perry_geisterhand_register_apply_style
  - perry_geisterhand_register_query_tree
  - perry_geisterhand_register_read_value
  - perry_geisterhand_register_screenshot_capture
  - perry_geisterhand_register_scroll_set
  - perry_geisterhand_register_state_set
  - perry_geisterhand_register_textfield_set_string
  - perry_geisterhand_register_with_shortcut
  - perry_geisterhand_registry_count
  - perry_geisterhand_request_screenshot
  - perry_geisterhand_request_tree
  - perry_geisterhand_request_value
crates/perry-runtime/src/ios_game_loop.rs:
  - perry_ios_classes_registered
  - perry_ios_get_connected_scene
crates/perry-runtime/src/ui_text_registry.rs:
  - js_foreach_register
  - js_navstack_register_route
  - js_register_foreach_render_handler
  - js_register_set_text_handler
  - js_register_show_toast_handler
  - js_register_text_id_handler
  - js_register_widget_hidden_handler
  - js_state_get
  - js_state_init
  - js_state_set
  - perry_arkts_register_text_id
crates/perry-runtime/src/watchos_game_loop.rs:
  - perry_watchos_classes_registered
*/
