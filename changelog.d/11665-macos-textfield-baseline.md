Fixed a macOS `TextField` whose text moved when editing started (#11661).
In single-line mode, AppKit draws a cell's idle text on the baseline of the
system font for the control size and ignores the cell's own font. The field
editor uses the cell's font. So with a custom font, the idle text sat 2pt high
at 16pt Helvetica, 3pt low at 10pt, and clipped at the top from about 20pt.
`PerryInsetTextFieldCell` now draws its interior with single-line mode off,
so the idle text sits where the field editor draws it. Single-line mode still
governs the field editor, so newline input still becomes spaces.
