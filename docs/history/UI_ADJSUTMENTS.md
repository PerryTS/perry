PLAN A: Perry Framework Changes

 All work in /Users/amlug/projects/perry. These are the perry-ui enhancements needed to make Pry possible.

 Current State

 Perry UI (v0.2.130) has: Text, Button, VStack, HStack, Spacer, Divider, TextField, Toggle, Slider, numeric-only State with reactive text
 binding, Cmd+Q quit, single window.

 Perry UI lacks: dynamic widget mutation, scrolling, text styling, clipboard, keyboard shortcuts, context menus, file dialogs, focus management.

 How to Add a New FFI Function (pattern for all changes below)

 Every new function requires changes in 3 places:
 1. Rust implementation — in crates/perry-ui-macos/src/widgets/*.rs or a new file
 2. FFI export — #[no_mangle] pub extern "C" in crates/perry-ui-macos/src/lib.rs
 3. Codegen — function declaration (~line 9196) AND match arm (~line 27315) in crates/perry-codegen/src/codegen.rs

 ---
 A.0: Dynamic Widget Tree (CRITICAL — must prove first)

 Risk gate: Validates that widgets can be created/removed from within event loop callbacks.
 FFI Function: perry_ui_widget_clear_children(parent: i64)
 Location: crates/perry-ui-macos/src/widgets/mod.rs
 Purpose: Remove all arrangedSubviews from an NSStackView
 ────────────────────────────────────────
 FFI Function: perry_ui_widget_set_hidden(handle: i64, hidden: f64)
 Location: same
 Purpose: Call setHidden() on an NSView
 Implementation notes:
 - clear_children: Get parent from WIDGETS vec, downcast to NSStackView, iterate arrangedSubviews(), call removeArrangedSubview() +
 removeFromSuperview() on each
 - set_hidden: Get view from WIDGETS vec, call setHidden(hidden != 0.0)

 Verification: Test TypeScript app — button press clears a VStack and adds new Text children dynamically.

 ---
 A.1: Text Mutation & Layout Control
 FFI Function: perry_ui_text_set_string(handle: i64, text_ptr: i64)
 Location: crates/perry-ui-macos/src/widgets/text.rs
 Purpose: Update NSTextField string value dynamically
 ────────────────────────────────────────
 FFI Function: perry_ui_vstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64
 Location: crates/perry-ui-macos/src/widgets/vstack.rs
 Purpose: VStack with configurable edge insets (current hardcodes 20px)
 ────────────────────────────────────────
 FFI Function: perry_ui_hstack_create_with_insets(spacing: f64, top: f64, left: f64, bottom: f64, right: f64) -> i64
 Location: crates/perry-ui-macos/src/widgets/hstack.rs
 Purpose: Same for HStack
 Implementation notes:
 - text_set_string: Extract StringHeader from NaN-boxed pointer, call setStringValue() on the NSTextField
 - *_with_insets: Same as existing create but pass custom NSEdgeInsets instead of hardcoded 20px

 ---
 A.2: ScrollView, Clipboard & Keyboard Shortcuts

 ScrollView (new widget)
 FFI Function: perry_ui_scrollview_create() -> i64
 Location: NEW: crates/perry-ui-macos/src/widgets/scrollview.rs
 Purpose: Create NSScrollView (vertical scroller, auto-hide)
 ────────────────────────────────────────
 FFI Function: perry_ui_scrollview_set_child(scroll: i64, child: i64)
 Location: same
 Purpose: Set the documentView
 Implementation notes:
 - Create NSScrollView, set hasVerticalScroller = true, autohidesScrollers = true
 - set_child: Set child as the documentView
 - Need to add "NSScrollView" feature to crates/perry-ui-macos/Cargo.toml objc2-app-kit features

 Clipboard
 ┌───────────────────────────────────────┬───────────────────────────────────────────┬─────────────────────────────────────────────────────────┐
 │             FFI Function              │                 Location                  │                         Purpose                         │
 ├───────────────────────────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
 │ perry_ui_clipboard_read() -> i64      │ NEW:                                      │ Read string from general pasteboard, returns NaN-boxed  │
 │                                       │ crates/perry-ui-macos/src/clipboard.rs    │ StringHeader ptr                                        │
 ├───────────────────────────────────────┼───────────────────────────────────────────┼─────────────────────────────────────────────────────────┤
 │ perry_ui_clipboard_write(text_ptr:    │ same                                      │ Write string to general pasteboard                      │
 │ i64)                                  │                                           │                                                         │
 └───────────────────────────────────────┴───────────────────────────────────────────┴─────────────────────────────────────────────────────────┘
 Implementation notes:
 - Use NSPasteboard::generalPasteboard(), read/write NSPasteboardTypeString
 - Return value: allocate StringHeader + UTF-8 data, NaN-box as STRING_TAG
 - Need to add "NSPasteboard" feature to Cargo.toml

 Keyboard Shortcuts
 FFI Function: perry_ui_add_keyboard_shortcut(key_ptr: i64, modifiers: f64, callback: f64)
 Location: crates/perry-ui-macos/src/app.rs
 Purpose: Add menu item with key equivalent + closure callback
 Implementation notes:
 - Extend setup_menu_bar() or add items after setup
 - Create NSMenuItem with keyEquivalent and keyEquivalentModifierMask
 - Modifiers: 1.0 = Cmd, 2.0 = Shift, 3.0 = Cmd+Shift (or use bitfield)
 - Custom target class (like PerryButtonTarget) that calls js_closure_call0

 ---
 A.3: Text Styling & Button Styling
 FFI Function: perry_ui_text_set_color(handle: i64, r: f64, g: f64, b: f64, a: f64)
 Location: crates/perry-ui-macos/src/widgets/text.rs
 Purpose: Set NSTextField textColor
 ────────────────────────────────────────
 FFI Function: perry_ui_text_set_font_size(handle: i64, size: f64)
 Location: same
 Purpose: Set font size
 ────────────────────────────────────────
 FFI Function: perry_ui_text_set_font_weight(handle: i64, weight: f64)
 Location: same
 Purpose: Set bold (1.0) / regular (0.0)
 ────────────────────────────────────────
 FFI Function: perry_ui_text_set_selectable(handle: i64, selectable: f64)
 Location: same
 Purpose: Make text selectable for copy
 ────────────────────────────────────────
 FFI Function: perry_ui_button_set_bordered(handle: i64, bordered: f64)
 Location: crates/perry-ui-macos/src/widgets/button.rs
 Purpose: Borderless mode for tree toggle buttons
 ────────────────────────────────────────
 FFI Function: perry_ui_button_set_title(handle: i64, title_ptr: i64)
 Location: same
 Purpose: Update button label dynamically
 Implementation notes:
 - set_color: NSColor::colorWithRed_green_blue_alpha(), call setTextColor(). Add "NSColor" feature.
 - set_font_size: NSFont::systemFontOfSize(), call setFont(). Add "NSFont" feature.
 - set_font_weight: NSFont::systemFontOfSize_weight() with NSFontWeightBold or NSFontWeightRegular
 - set_bordered: Call setBordered() on NSButton
 - set_title: Extract StringHeader, call setTitle() on NSButton

 ---
 A.4: Focus & Scroll-To
 FFI Function: perry_ui_textfield_focus(handle: i64)
 Location: crates/perry-ui-macos/src/widgets/textfield.rs
 Purpose: Make text field first responder
 ────────────────────────────────────────
 FFI Function: perry_ui_scrollview_scroll_to(scroll: i64, child: i64)
 Location: crates/perry-ui-macos/src/widgets/scrollview.rs
 Purpose: Scroll to make a child widget visible
 ────────────────────────────────────────
 FFI Function: perry_ui_scrollview_get_offset(scroll: i64) -> f64
 Location: same
 Purpose: Get current Y scroll offset
 ────────────────────────────────────────
 FFI Function: perry_ui_scrollview_set_offset(scroll: i64, offset: f64)
 Location: same
 Purpose: Restore scroll position after rebuild
 Implementation notes:
 - focus: Get NSTextField from WIDGETS, get its window, call makeFirstResponder()
 - scroll_to: Get child NSView, call scrollRectToVisible() on it
 - get/set_offset: Access contentView.bounds.origin.y

 ---
 A.5: Context Menus, File Dialog & Window Sizing

 Context Menus
 FFI Function: perry_ui_context_menu_create() -> i64
 Location: NEW: crates/perry-ui-macos/src/menu.rs
 Purpose: Create NSMenu
 ────────────────────────────────────────
 FFI Function: perry_ui_context_menu_add_item(menu: i64, title_ptr: i64, callback: f64)
 Location: same
 Purpose: Add NSMenuItem with callback
 ────────────────────────────────────────
 FFI Function: perry_ui_widget_set_context_menu(widget: i64, menu: i64)
 Location: same
 Purpose: Set menu property on NSView
 File Dialog
 FFI Function: perry_ui_file_open_dialog(allowed_types_ptr: i64, callback: f64)
 Location: NEW: crates/perry-ui-macos/src/file_dialog.rs
 Purpose: Show NSOpenPanel, call callback with selected path
 Implementation notes:
 - Create NSOpenPanel, set allowedContentTypes for .json
 - Run modal, get selected URL, convert to path string
 - Call js_closure_call1(callback, nanboxed_path_string)

 Window Sizing
 ┌─────────────────────────────────────────────────────┬──────────────────────────────────┬─────────────────────────┐
 │                    FFI Function                     │             Location             │         Purpose         │
 ├─────────────────────────────────────────────────────┼──────────────────────────────────┼─────────────────────────┤
 │ perry_ui_app_set_min_size(app: i64, w: f64, h: f64) │ crates/perry-ui-macos/src/app.rs │ Set window minimum size │
 ├─────────────────────────────────────────────────────┼──────────────────────────────────┼─────────────────────────┤
 │ perry_ui_app_set_max_size(app: i64, w: f64, h: f64) │ same                             │ Set window maximum size │
 └─────────────────────────────────────────────────────┴──────────────────────────────────┴─────────────────────────┘
 ---
 Summary: All Perry FFI Functions
 Phase: A.0
 Functions: widget_clear_children, widget_set_hidden
 New Files: —
 ────────────────────────────────────────
 Phase: A.1
 Functions: text_set_string, vstack_create_with_insets, hstack_create_with_insets
 New Files: —
 ────────────────────────────────────────
 Phase: A.2
 Functions: scrollview_create, scrollview_set_child, clipboard_read, clipboard_write, add_keyboard_shortcut
 New Files: scrollview.rs, clipboard.rs
 ────────────────────────────────────────
 Phase: A.3
 Functions: text_set_color, text_set_font_size, text_set_font_weight, text_set_selectable, button_set_bordered, button_set_title
 New Files: —
 ────────────────────────────────────────
 Phase: A.4
 Functions: textfield_focus, scrollview_scroll_to, scrollview_get_offset, scrollview_set_offset
 New Files: —
 ────────────────────────────────────────
 Phase: A.5
 Functions: context_menu_create, context_menu_add_item, widget_set_context_menu, file_open_dialog, app_set_min_size, app_set_max_size
 New Files: menu.rs, file_dialog.rs
 Total: 24 new FFI functions, 4 new Rust files, ~50 codegen declarations, ~100 codegen match arms

 Cargo.toml feature additions: "NSScrollView", "NSPasteboard", "NSFont", "NSColor", "NSOpenPanel", "NSMenu", "NSMenuItem"


