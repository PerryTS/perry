Add `widgetSetMaxWidth(widget, maxWidth)` for responsive centered content columns.
On macOS, Auto Layout fills available width up to the cap while preserving padding,
stack alignment, and widget identity. Caps can be set before or after insertion and
updated in place. JavaScript and WebAssembly web targets use CSS max-width and auto
margins. Other backends export a documented no-op for portable compilation.

Fixes #10167. Native layout regressions cover resizing, padded parents and content,
repeated updates, insertion order, fill alignment, hide/show, reparenting,
scroll views, and app-root placement. Native, JavaScript, and WebAssembly codegen
tests cover the new runtime call.
