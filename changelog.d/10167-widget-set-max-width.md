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

Per-backend behaviour, all reproducing fill-below-cap + cap-and-center:

- **macOS / iOS / tvOS / visionOS** — three Auto Layout constraints:
  `width <= maxWidth` (required), `width == superview.width` at priority 999
  (grow to fill until the cap binds), `centerX == superview.centerX`. Idempotent.
- **GTK4** — a `MaxWidthBin` widget subclass (GTK4 has no max-width property);
  its `measure`/`size_allocate` cap the child at the width and center it. The
  child is re-parented through the bin inside its `GtkBox`.
- **Windows (Win32)** — a `max_width` field honoured at layout time (clamp the
  cross-axis width and center by growing the gutters), DPI-scaled.
- **WinUI** — `MaxWidth` + `HorizontalAlignment::Stretch` via `windows_reactor`.
- **Android** — a `PerryMaxWidthLayout` (`FrameLayout` whose `onMeasure` clamps
  the child to the cap and centers it); `widgetSetMaxWidth` re-parents the child
  through it via JNI.
- **watchOS** — the SwiftUI host applies `.frame(maxWidth:, alignment: .center)`
  from a new `frame_max_width` introspection field.
- **web (JS + WASM runtimes)** — `width: 100%` + `max-width` + `margin: auto`.
- **ArkTS (HarmonyOS)** — `.width('100%').constraintSize({ maxWidth })
  .alignSelf(ItemAlign.Center)`.

Also exposed as a `maxWidth` prop in the `perry-solid` renderer, documented on
the UI styling page, and covered by a runnable doc-example
(`ui/layout/max_width_centered.ts`) that the iOS simulator harness runs.

Verification (see the PR description for the full per-backend matrix and the
environmental limits behind each): macOS pixel-measured (320pt cap, equal
gutters); iOS run clean on a real simulator; web measured in a browser (the
same 320/290/290); the GTK4 `MaxWidthBin` measure/allocate pixel-proven with a
standalone GTK4 program. Windows/WinUI compile in CI; Android, tvOS, visionOS,
watchOS and the GTK4 integration have no build or run path in the authoring
environment and rest on review plus their platform builds.
