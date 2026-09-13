Fixed macOS `TextField` wrapping to multiple lines when its text was wider than
the field. `perry-ui-macos`'s `create()` built the field from
`NSTextField::textFieldWithString`, whose cell wraps and grows, and never
overrode it — so a fixed-width field grew tall instead of scrolling, unlike
every other backend. The cell is now configured for single-line editing
(`usesSingleLineMode`, `scrollable`, `wraps = false`, clipping line-break mode),
matching iOS / GTK4 / Android / Windows. Multiline input keeps its own widget,
`TextArea`.
