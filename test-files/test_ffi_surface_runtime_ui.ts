// Runtime UI and platform FFI surface inventory.
//
// This fixture is intentionally executable by the normal parity runner,
// but its main purpose is to keep TS-side coverage accounting attached
// to related public FFI shims. Move @covers entries from this
// inventory into behavioral tests as each area gets deeper compatibility
// coverage.
//
// Inventory entries: 121 unique FFI names, 126 declarations.

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
crates/perry-runtime/src/jsx.rs:
  - js_jsxs
crates/perry-runtime/src/media_playback.rs:
  - perry_media_create_player
  - perry_media_destroy
  - perry_media_get_current_time
  - perry_media_get_duration
  - perry_media_get_state
  - perry_media_is_playing
  - perry_media_on_state_change
  - perry_media_on_time_update
  - perry_media_pause
  - perry_media_play
  - perry_media_seek
  - perry_media_set_now_playing
  - perry_media_set_rate
  - perry_media_set_volume
  - perry_media_stop
crates/perry-runtime/src/tui/ffi.rs:
  - js_perry_tui_animated_spinner
  - js_perry_tui_box_add_children_array
  - js_perry_tui_box_set_align_items
  - js_perry_tui_box_set_flex_basis
  - js_perry_tui_box_set_flex_basis_pct
  - js_perry_tui_box_set_flex_direction
  - js_perry_tui_box_set_flex_grow
  - js_perry_tui_box_set_flex_shrink
  - js_perry_tui_box_set_gap
  - js_perry_tui_box_set_height
  - js_perry_tui_box_set_height_pct
  - js_perry_tui_box_set_justify_content
  - js_perry_tui_box_set_padding
  - js_perry_tui_box_set_padding_each
  - js_perry_tui_box_set_width
  - js_perry_tui_box_set_width_pct
  - js_perry_tui_enter
  - js_perry_tui_input
  - js_perry_tui_input_at
  - js_perry_tui_list
  - js_perry_tui_progress_bar
  - js_perry_tui_render
  - js_perry_tui_select
  - js_perry_tui_spacer
  - js_perry_tui_spinner
  - js_perry_tui_table
  - js_perry_tui_tabs
  - js_perry_tui_text_area
  - js_perry_tui_text_styled
crates/perry-runtime/src/tui/hooks.rs:
  - js_perry_tui_app_exit
  - js_perry_tui_app_wait_until_exit
  - js_perry_tui_focus
  - js_perry_tui_focus_manager_focus
  - js_perry_tui_focus_manager_focus_next
  - js_perry_tui_focus_manager_focus_previous
  - js_perry_tui_focus_next
  - js_perry_tui_focus_previous
  - js_perry_tui_ref_get
  - js_perry_tui_ref_set
  - js_perry_tui_stdout_columns
  - js_perry_tui_stdout_rows
  - js_perry_tui_stdout_write
  - js_perry_tui_use_app
  - js_perry_tui_use_effect
  - js_perry_tui_use_focus
  - js_perry_tui_use_focus_manager
  - js_perry_tui_use_memo
  - js_perry_tui_use_ref
  - js_perry_tui_use_state
  - js_perry_tui_use_state_set
  - js_perry_tui_use_state_slot
  - js_perry_tui_use_state_tuple
  - js_perry_tui_use_stdout
  - js_perry_tui_wait_until_exit
  - perry_tui_state_setter_trampoline
crates/perry-runtime/src/tui/input.rs:
  - js_perry_tui_exit
  - js_perry_tui_use_input
crates/perry-runtime/src/tui/run.rs:
  - js_perry_tui_run
crates/perry-runtime/src/tui/state.rs:
  - js_perry_tui_state_alloc
  - js_perry_tui_state_get
  - js_perry_tui_state_set
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
