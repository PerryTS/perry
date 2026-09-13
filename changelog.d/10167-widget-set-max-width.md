**New `widgetSetMaxWidth(widget, maxWidth)` UI layout API** — the CSS
`max-width` + `margin: auto` pattern for a centered content column. Below the
cap the widget fills the available width; at and above `maxWidth` it stops and
centers, so the side gutters grow with the window. Before this the public width
surface offered only `widgetSetWidth` (an exact `==` width) and
`widgetMatchParentWidth` (fill), with no way to express "grow to a cap, then
stop and center" (issue #10167).

Wired through the central `PERRY_UI_TABLE` dispatch as
`perry_ui_widget_set_max_width` (`args: [Widget, F64]`), so the LLVM backend
lowers it generically and the JS/WASM codegen resolve it via the existing
`ui_method_to_runtime` fall-through — no per-backend codegen change.

Per-backend behaviour:

- **macOS / iOS / tvOS / visionOS** — real Auto Layout, three constraints on the
  view: `width <= maxWidth` (required), `width == superview.width` at priority
  999 (grow to fill until the cap binds), and `centerX == superview.centerX`
  (center once capped). Idempotent: a prior set is deactivated before re-apply.
  The widget must already have a superview when called, matching
  `widgetMatchParentWidth`.
- **GTK4** — `hexpand(true)` + `halign(Center)` + `set_size_request(maxWidth,
  -1)`; GTK has no true max-width property, so this fills to the requested cap
  and centers beyond it.
- **Windows (Win32)** — a `max_width` field on the widget entry, honoured at
  layout time (clamp the cross-axis width to the cap and center by growing the
  gutters). DPI-scaled, mirroring `set_width`.
- **WinUI** — `MaxWidth` + `HorizontalAlignment::Stretch` through
  `windows_reactor`, which is exactly the fill-then-center contract.
- **Android** — fill (`MATCH_PARENT`) + `CENTER_HORIZONTAL`; a generic `View`
  has no max-width and there is no measure-time hook at this call site, so the
  hard cap is not enforced yet (documented in the impl). The FFI symbol exists so
  the target links.
- **watchOS** — documented no-op (WatchKit has no Auto Layout), matching its
  `match_parent_width` stub.
- **web (JS + WASM runtimes)** — `el.style.maxWidth` + `margin-left/right: auto`,
  a faithful reproduction of the behaviour.
- **ArkTS (HarmonyOS)** — emits `.width('100%').constraintSize({ maxWidth })
  .alignSelf(ItemAlign.Center)`, the nearest ArkUI idiom.

Also exposed as a `maxWidth` prop in the `perry-solid` renderer, documented on
the UI styling page, and covered by a runnable doc-example snippet.

Incidental fix: the Android `set_max_width` shim uses the correct `i64` handle
ABI (`decode`d via `match_parent_width`'s path), not the `f64` handle its
existing `set_width` shim still uses — a latent ABI mismatch against the
`ArgKind::Widget` codegen lowering, noted for a separate cleanup.
