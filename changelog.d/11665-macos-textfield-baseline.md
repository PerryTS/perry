Fixed a macOS `TextField` whose text moved when editing started (#11661).
`TextField`, `SecureField`, `Text` and `AttributedText` replaced the cell that
AppKit builds with a new `PerryInsetTextFieldCell`, so that `setPadding` could
inset their text. The new cell lost the factory setup. `TextField` then set
its one-line properties again and turned on `usesSingleLineMode`. In that mode
AppKit draws idle text on the baseline of the system font for the control
size, so a custom font sat 2pt high at 16pt Helvetica and clipped from about
20pt. `PerryInsetTextField` and `PerryInsetSecureTextField` now override
`cellClass`, so `textFieldWithString:` and `labelWithString:` build the inset
cell with the factory setup. The cell swap, the property restore for labels,
and the one-line properties are gone. `SecureField` is now one line and
scrolls, as `TextField` does. Option-Return in a `TextField` inserts a
newline, as it does in a stock `NSTextField`.
