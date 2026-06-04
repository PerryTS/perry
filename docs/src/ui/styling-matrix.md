# perry/ui styling matrix

Auto-generated from `crates/perry-ui/src/styling_matrix.rs` by `scripts/run_ui_styling_matrix.sh`. Do not edit by hand — CI fails if this file drifts from the source-of-truth.

Legend: `✓` Wired (real native impl), `~` Stub (symbol exists, no-op), `✗` Missing (FFI symbol not exported), `—` Not applicable to this platform.

## Generic widget setters (apply to any widget)

| Prop | FFI symbol | macOS | iOS | tvOS | visionOS | watchOS | Android | GTK4 | Windows | Web |
|---|---|---|---|---|---|---|---|---|---|---|
| `background_color` | `perry_ui_widget_set_background_color` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `background_gradient` | `perry_ui_widget_set_background_gradient` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `border_color` | `perry_ui_widget_set_border_color` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `border_width` | `perry_ui_widget_set_border_width` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `corner_radius` | `perry_ui_widget_set_corner_radius` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `edge_insets` | `perry_ui_widget_set_edge_insets` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `opacity` | `perry_ui_widget_set_opacity` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `tooltip` | `perry_ui_widget_set_tooltip` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `hidden` | `perry_ui_set_widget_hidden` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `enabled` | `perry_ui_widget_set_enabled` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `control_size` | `perry_ui_widget_set_control_size` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `hugging` | `perry_ui_widget_set_hugging` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `width` | `perry_ui_widget_set_width` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `height` | `perry_ui_widget_set_height` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `match_parent_width` | `perry_ui_widget_match_parent_width` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `match_parent_height` | `perry_ui_widget_match_parent_height` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `on_click` | `perry_ui_widget_set_on_click` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `on_double_click` | `perry_ui_widget_set_on_double_click` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `on_hover` | `perry_ui_widget_set_on_hover` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `on_mouse_down` | `perry_ui_widget_set_on_mouse_down` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `on_mouse_up` | `perry_ui_widget_set_on_mouse_up` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `on_mouse_move` | `perry_ui_widget_set_on_mouse_move` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `animate_opacity` | `perry_ui_widget_animate_opacity` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `animate_position` | `perry_ui_widget_animate_position` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `context_menu` | `perry_ui_widget_set_context_menu` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `shadow` | `perry_ui_widget_set_shadow` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## `button` widget

| Prop | FFI symbol | macOS | iOS | tvOS | visionOS | watchOS | Android | GTK4 | Windows | Web |
|---|---|---|---|---|---|---|---|---|---|---|
| `text_color` | `perry_ui_button_set_text_color` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `content_tint_color` | `perry_ui_button_set_content_tint_color` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `bordered` | `perry_ui_button_set_bordered` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `image_position` | `perry_ui_button_set_image_position` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## `image` widget

| Prop | FFI symbol | macOS | iOS | tvOS | visionOS | watchOS | Android | GTK4 | Windows | Web |
|---|---|---|---|---|---|---|---|---|---|---|
| `tint` | `perry_ui_image_set_tint` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `size` | `perry_ui_image_set_size` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## `stack` widget

| Prop | FFI symbol | macOS | iOS | tvOS | visionOS | watchOS | Android | GTK4 | Windows | Web |
|---|---|---|---|---|---|---|---|---|---|---|
| `alignment` | `perry_ui_stack_set_alignment` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `distribution` | `perry_ui_stack_set_distribution` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `detaches_hidden` | `perry_ui_stack_set_detaches_hidden` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## `text` widget

| Prop | FFI symbol | macOS | iOS | tvOS | visionOS | watchOS | Android | GTK4 | Windows | Web |
|---|---|---|---|---|---|---|---|---|---|---|
| `color` | `perry_ui_text_set_color` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `font_size` | `perry_ui_text_set_font_size` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `font_weight` | `perry_ui_text_set_font_weight` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `font_family` | `perry_ui_text_set_font_family` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `selectable` | `perry_ui_text_set_selectable` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `wraps` | `perry_ui_text_set_wraps` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `text_alignment` | `perry_ui_text_set_text_alignment` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `decoration` | `perry_ui_text_set_decoration` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## `textfield` widget

| Prop | FFI symbol | macOS | iOS | tvOS | visionOS | watchOS | Android | GTK4 | Windows | Web |
|---|---|---|---|---|---|---|---|---|---|---|
| `background_color` | `perry_ui_textfield_set_background_color` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `text_color` | `perry_ui_textfield_set_text_color` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `font_size` | `perry_ui_textfield_set_font_size` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |
| `borderless` | `perry_ui_textfield_set_borderless` | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ | ✓ |

## Summary

| Platform | Wired | Stub | Missing | Not applicable |
|---|---|---|---|---|
| macOS | 47 | 0 | 0 | 0 |
| iOS | 47 | 0 | 0 | 0 |
| tvOS | 47 | 0 | 0 | 0 |
| visionOS | 47 | 0 | 0 | 0 |
| watchOS | 47 | 0 | 0 | 0 |
| Android | 47 | 0 | 0 | 0 |
| GTK4 | 47 | 0 | 0 | 0 |
| Windows | 47 | 0 | 0 | 0 |
| Web | 47 | 0 | 0 | 0 |

