Fixed a macOS `TextField`, `SecureField` or `Text` that overhung its stack by
2pt on each side (#11739). AppKit gives a label-shaped text field 2pt
alignment rect insets on each side, and Auto Layout pins the alignment rect to
the stack. The frame, and the layer border and background drawn on it, grew
2pt past each edge. A `Text` always had the insets. A `TextField` or
`SecureField` got them once `textfieldSetBackgroundColor` turned off its cell
background.

`PerryTextField`, `PerrySecureTextField` and `PerryLabel` now return zero
`alignmentRectInsets`, so each one fills the width it is pinned to whatever it
draws. A stock label's insets hide the 2pt side padding that its text layout
adds. `PerryLabel`'s cell draws over that padding, so a `Text` keeps its text
where a stock label draws it, as a web `<span>` does, and keeps its intrinsic
width.
