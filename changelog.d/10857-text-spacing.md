Added `textSetLetterSpacing(widget, points)` and `textSetLineHeight(widget, multiple)`
for macOS Text labels and web UI. AppKit stores both as text attributes and
preserves them when the label text, color, font, or alignment changes. Passing
zero spacing or a line-height multiple of one restores the default.
