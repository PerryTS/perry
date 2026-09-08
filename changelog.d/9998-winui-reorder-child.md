The Windows WinUI backend now implements child reordering instead of failing
to compile when the shared UI dispatcher calls `reorder_child`. Fluent trees
move the existing child in their model and request a render; processes using
the Win32 fallback keep delegating to the existing Win32 implementation.
