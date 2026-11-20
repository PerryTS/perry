### Added

- **`WheelPicker` — a native scrolling wheel/drum selector (#5873).** Renders a
  real platform wheel where one exists (`UIPickerView` on iOS/tvOS/visionOS,
  `NumberPicker` on Android) and a scroll-capable native control elsewhere
  (macOS, Windows, GTK4, watchOS), so the widget is available on every target
  rather than only the ones with a first-class drum control.

  Dispatch is wired across all four backends — native, JS (`web_runtime.js`),
  WASM (`ui_method_map.rs`), and ArkTS — plus the shared UI dispatch table, HIR
  lowering, the API manifest, `perry.d.ts`, and the widget docs, so the same
  TypeScript source compiles on each.

  Each platform's implementation parks its per-handle `on_change` closure in a
  thread-local `CALLBACKS` table, matching the existing `COMBOBOX_CALLBACKS`
  pattern that every other callback-bearing widget uses. Those tables are
  NaN-boxed-callback holders that no registered GC scanner reaches, so the two
  new ones (iOS, visionOS) are pinned in the `frontier` ratchet of
  `scripts/gc_runtime_root_holders.json` alongside the other 112 UI callback
  tables — deliberately, not silently: registering per-crate scanners over the
  whole `perry-ui*` tier remains the real fix and the ratchet may only shrink.
  The sibling `SELECTED` tables are pinned for the same shape reason but hold
  only row indices, never a heap value.
