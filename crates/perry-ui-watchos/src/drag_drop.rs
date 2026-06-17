//! Drag & drop FFI (issue #4773).
//!
//! Widget-level drag/drop setters exported by every `perry-ui-*` backend so
//! that `widgetOnDrop` / `widgetSetDrag*` compile and link on every
//! `--target`: codegen emits a single symbol name regardless of platform (see
//! `crates/perry-dispatch/src/ui_table.rs`), so the symbol must exist in each
//! platform's static library or the link fails.
//!
//! This backend currently provides no-op implementations. Native behavior is
//! landed per platform as follow-up work (macOS AppKit `NSDraggingDestination`
//! / `NSDraggingSource`, UIKit `UIDropInteraction` / `UIDragInteraction`,
//! GTK4 `GtkDropTarget` / `GtkDragSource`, Win32 `IDropTarget` / `DoDragDrop`,
//! Android `View.OnDragListener` / `startDragAndDrop`).

/// Register `widget` as a drop destination. `callback` (a NaN-boxed closure)
/// is invoked with a `{ text?, files?, urls? }` object describing the payload
/// when text, files, or URLs are dropped onto the widget.
#[no_mangle]
pub extern "C" fn perry_ui_widget_on_drop(_widget: i64, _callback: f64) {}

/// Register `widget` as a drag source offering plain text. `provider` (a
/// NaN-boxed closure) returns the text payload when a drag begins.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_text(_widget: i64, _provider: f64) {}

/// Register `widget` as a drag source offering a file. `provider` returns the
/// absolute path of the file to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_file(_widget: i64, _provider: f64) {}

/// Register `widget` as a drag source offering a web URL. `provider` returns
/// the URL string to carry.
#[no_mangle]
pub extern "C" fn perry_ui_widget_set_drag_url(_widget: i64, _provider: f64) {}
