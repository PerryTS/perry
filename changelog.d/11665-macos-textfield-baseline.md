Fixed a macOS `TextField` whose text moved when editing started (#11661).
`TextField`, `SecureField`, `Text` and `AttributedText` replaced the cell that
AppKit builds with a new `PerryInsetTextFieldCell`, so that `setPadding` could
inset their text. The new cell lost the factory setup. `TextField` then set
its one-line properties again and turned on `usesSingleLineMode`. In that mode
AppKit draws idle text on the baseline of the system font for the control
size, so a custom font sat 2pt high at 16pt Helvetica and clipped from about
20pt. `PerryTextField` and `PerrySecureTextField` now override
`cellClass`, so `textFieldWithString:` and `labelWithString:` build the inset
cell with the factory setup. The cell swap, the property restore for labels,
and the one-line properties are gone. `SecureField` is now one line and
scrolls, as `TextField` does.

`TextField` and `SecureField` hold one line, as a web `<input>` does, without
single-line mode. A value set in code drops its line breaks. Typed, pasted and
dropped text turns each line break into one space. Option-Return and
Control-Return insert nothing, and Return still submits.

`setPadding` on a `TextField` or `SecureField` now works as CSS padding does
on the web. A padded field with a bezel or a border moved its text by the
padding when editing started, because AppKit applied the cell's
`drawingRectForBounds:` padding to the editing frame as well. The cell now pads
only the frames that AppKit hands it to draw and to edit, so the padding
applies once for every border style. `textfieldSetBackgroundColor` paints the
field's layer, so the background fills the padding too. The cell's own
background filled only the area inside the padding.

Layer colours reach CoreAnimation as a typed `CGColor` pointer. As a `c_void`
pointer, `widgetSetBackgroundColor`, border colours and shadow colours
panicked in a debug build's message check.
