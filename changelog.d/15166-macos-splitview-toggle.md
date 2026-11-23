macOS: `SplitView` is now implemented with `NSSplitView` (it was a stub that
returned handle 0, so only the first child showed, across the full width). The
first pane starts at 260pt, keeps its width when the window resizes, and the
divider can be dragged. Toggle, Slider and Picker callbacks no longer panic
with "RefCell already borrowed" when the handler creates another widget of the
same kind.
