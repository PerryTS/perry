**UI padding API:** `setPadding` is now the canonical imperative API on every
backend and accepts both `(widget, value)` and `(widget, top, left, bottom,
right)`. The `widgetSetEdgeInsets` spelling remains as a deprecated alias for
the compatibility window. Web, WASM, native, and HarmonyOS now agree on side
ordering, including asymmetric inline `padding` objects.
