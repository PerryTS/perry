### fix(gtk4): initialize GTK before creating Canvas

Perry applications commonly construct their widget tree before calling
`App()`. Unlike the other GTK widget constructors,
`crates/perry-ui-gtk4/src/widgets/canvas.rs` previously called
`DrawingArea::new()` without first invoking the shared GTK initializer, so that
normal ordering aborted with “GTK has not been initialized.”

`Canvas()` now calls `ensure_gtk_init()` before `DrawingArea::new()`. A
source-order regression test covers the fixed ordering and proves that the
pre-fix sequence is rejected. Fixes #7995.
