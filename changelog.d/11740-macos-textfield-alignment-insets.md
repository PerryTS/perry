Fixed a macOS `TextField` or `SecureField` that overhung its stack by 2pt on
each side (#11739). `textfieldSetBackgroundColor` paints the field's layer and
turns off the cell background. AppKit treats a field with no bezel, no border,
and no cell background as a label, and gives it a label's 2pt side alignment
insets. Auto Layout pins the alignment rect to the stack, so the frame, and
the layer border and background drawn on it, grew 2pt past each edge.
`PerryTextField` and `PerrySecureTextField` now return zero
`alignmentRectInsets`, so a field fills the width it is pinned to whatever it
draws.
